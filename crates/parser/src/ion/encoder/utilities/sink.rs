#[cfg(not(all(target_arch = "wasm32", not(target_os = "wasi"))))]
use std::{
    fs::{self, File, OpenOptions},
    io::{BufWriter, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use crate::ion::{IonError, IonResult};

pub trait WriteBytes {
    fn write(&mut self, bytes: &[u8]) -> IonResult<()>;
    fn patch(&mut self, at: u64, bytes: &[u8]) -> IonResult<()>;
    fn position(&mut self) -> IonResult<u64>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SectionStorage {
    Memory,
    Disk,
}

#[cfg(not(all(target_arch = "wasm32", not(target_os = "wasi"))))]
pub(crate) fn make_chunk(
    mode: SectionStorage,
    tag: &str,
    capacity: usize,
) -> IonResult<SectionChunk> {
    match mode {
        SectionStorage::Memory => Ok(SectionChunk::memory(capacity)),
        SectionStorage::Disk => SectionChunk::disk(&std::env::temp_dir(), tag),
    }
}

#[cfg(all(target_arch = "wasm32", not(target_os = "wasi")))]
pub(crate) fn make_chunk(
    _mode: SectionStorage,
    _tag: &str,
    capacity: usize,
) -> IonResult<SectionChunk> {
    Ok(SectionChunk::memory(capacity))
}

#[cfg(not(all(target_arch = "wasm32", not(target_os = "wasi"))))]
pub struct FileWriter {
    writer: BufWriter<File>,
}

#[cfg(not(all(target_arch = "wasm32", not(target_os = "wasi"))))]
impl FileWriter {
    pub fn open(path: &str) -> IonResult<Self> {
        Self::open_path(Path::new(path))
    }

    pub fn open_path(path: &Path) -> IonResult<Self> {
        let file = File::create(path).map_err(|err| {
            IonError::from(format!(
                "cannot create output file '{}': {err}",
                path.display()
            ))
        })?;
        Ok(Self {
            writer: BufWriter::with_capacity(8 * 1024 * 1024, file),
        })
    }

    pub fn flush(&mut self) -> IonResult<()> {
        use std::io::Write as StdWrite;
        self.writer
            .flush()
            .map_err(|err| IonError::from(format!("flush error: {err}")))
    }
}

#[cfg(not(all(target_arch = "wasm32", not(target_os = "wasi"))))]
impl WriteBytes for FileWriter {
    fn write(&mut self, bytes: &[u8]) -> IonResult<()> {
        std::io::Write::write_all(&mut self.writer, bytes)
            .map_err(|err| IonError::from(format!("write error: {err}")))
    }

    fn patch(&mut self, at: u64, bytes: &[u8]) -> IonResult<()> {
        use std::io::Write as StdWrite;
        self.writer
            .flush()
            .map_err(|err| IonError::from(format!("flush error: {err}")))?;
        let resume_position = self
            .writer
            .stream_position()
            .map_err(|err| IonError::from(format!("position error: {err}")))?;
        self.writer
            .seek(SeekFrom::Start(at))
            .map_err(|err| IonError::from(format!("seek error: {err}")))?;
        std::io::Write::write_all(&mut self.writer, bytes)
            .map_err(|err| IonError::from(format!("patch write error: {err}")))?;
        self.writer
            .seek(SeekFrom::Start(resume_position))
            .map_err(|err| IonError::from(format!("seek-resume error: {err}")))?;
        Ok(())
    }

    fn position(&mut self) -> IonResult<u64> {
        use std::io::Write as StdWrite;
        self.writer
            .flush()
            .map_err(|err| IonError::from(format!("flush error: {err}")))?;
        self.writer
            .stream_position()
            .map_err(|err| IonError::from(format!("position error: {err}")))
    }
}

#[cfg(not(all(target_arch = "wasm32", not(target_os = "wasi"))))]
pub struct TempFile {
    path: PathBuf,
    delete_on_drop: bool,
}

#[cfg(not(all(target_arch = "wasm32", not(target_os = "wasi"))))]
impl TempFile {
    pub fn new(output_path: &Path) -> IonResult<Self> {
        let output_folder = output_path.parent().unwrap_or_else(|| Path::new("."));
        let output_name = output_path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("ion");
        let process_id = std::process::id();
        let time_id = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|time| time.as_nanos())
            .unwrap_or(0);
        let temp_name = format!(".{output_name}.tmp.{process_id}.{time_id}");
        Ok(Self {
            path: output_folder.join(temp_name),
            delete_on_drop: true,
        })
    }

    pub fn in_dir(dir: &Path, tag: &str) -> IonResult<Self> {
        let process_id = std::process::id();
        let time_id = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|time| time.as_nanos())
            .unwrap_or(0);
        let temp_name = format!(".ionic-section-chunk.{tag}.{process_id}.{time_id}");
        Ok(Self {
            path: dir.join(temp_name),
            delete_on_drop: true,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn move_to(mut self, output_path: &Path) -> IonResult<()> {
        fs::rename(&self.path, output_path).map_err(|err| {
            IonError::from(format!(
                "cannot move '{}' to '{}': {err}",
                self.path.display(),
                output_path.display()
            ))
        })?;
        self.delete_on_drop = false;
        Ok(())
    }
}

#[cfg(not(all(target_arch = "wasm32", not(target_os = "wasi"))))]
impl Drop for TempFile {
    fn drop(&mut self) {
        if self.delete_on_drop {
            let _ = fs::remove_file(&self.path);
        }
    }
}

#[cfg(not(all(target_arch = "wasm32", not(target_os = "wasi"))))]
pub(crate) struct DiskSectionChunk {
    writer: BufWriter<File>,
    temp: TempFile,
    len: u64,
}

pub(crate) enum SectionChunk {
    Memory(Vec<u8>),
    #[cfg(not(all(target_arch = "wasm32", not(target_os = "wasi"))))]
    Disk(DiskSectionChunk),
}

impl SectionChunk {
    pub(crate) fn memory(capacity: usize) -> Self {
        SectionChunk::Memory(Vec::with_capacity(capacity))
    }

    #[cfg(not(all(target_arch = "wasm32", not(target_os = "wasi"))))]
    pub(crate) fn disk(dir: &Path, tag: &str) -> IonResult<Self> {
        let temp = TempFile::in_dir(dir, tag)?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(temp.path())
            .map_err(|err| {
                IonError::from(format!(
                    "cannot create section chunk file '{}': {err}",
                    temp.path().display()
                ))
            })?;
        Ok(SectionChunk::Disk(DiskSectionChunk {
            writer: BufWriter::with_capacity(1 << 20, file),
            temp,
            len: 0,
        }))
    }

    pub(crate) fn write(&mut self, bytes: &[u8]) -> IonResult<()> {
        match self {
            SectionChunk::Memory(buffer) => buffer.extend_from_slice(bytes),
            #[cfg(not(all(target_arch = "wasm32", not(target_os = "wasi"))))]
            SectionChunk::Disk(disk) => {
                std::io::Write::write_all(&mut disk.writer, bytes)
                    .map_err(|err| IonError::from(format!("section chunk write error: {err}")))?;
                disk.len += bytes.len() as u64;
            }
        }
        Ok(())
    }

    pub(crate) fn len(&self) -> u64 {
        match self {
            SectionChunk::Memory(buffer) => buffer.len() as u64,
            #[cfg(not(all(target_arch = "wasm32", not(target_os = "wasi"))))]
            SectionChunk::Disk(disk) => disk.len,
        }
    }

    #[cfg(test)]
    pub(crate) fn as_slice(&self) -> Option<&[u8]> {
        match self {
            SectionChunk::Memory(buffer) => Some(buffer),
            #[cfg(not(all(target_arch = "wasm32", not(target_os = "wasi"))))]
            SectionChunk::Disk(_) => None,
        }
    }

    pub(crate) fn into_vec(self) -> IonResult<Vec<u8>> {
        match self {
            SectionChunk::Memory(buffer) => Ok(buffer),
            #[cfg(not(all(target_arch = "wasm32", not(target_os = "wasi"))))]
            SectionChunk::Disk(disk) => {
                let DiskSectionChunk {
                    writer,
                    temp,
                    len: _,
                } = disk;
                let mut file = writer
                    .into_inner()
                    .map_err(|err| IonError::from(format!("section chunk flush error: {err}")))?;
                file.seek(SeekFrom::Start(0))
                    .map_err(|err| IonError::from(format!("section chunk seek error: {err}")))?;
                let mut buffer = Vec::new();
                file.read_to_end(&mut buffer)
                    .map_err(|err| IonError::from(format!("section chunk read error: {err}")))?;
                drop(temp);
                Ok(buffer)
            }
        }
    }

}

impl WriteBytes for Vec<u8> {
    fn write(&mut self, bytes: &[u8]) -> IonResult<()> {
        self.extend_from_slice(bytes);
        Ok(())
    }

    fn patch(&mut self, at: u64, bytes: &[u8]) -> IonResult<()> {
        let start = at as usize;
        let end = start + bytes.len();
        self.get_mut(start..end)
            .ok_or_else(|| {
                IonError::from(format!(
                    "patch: range {start}..{end} out of bounds"
                ))
            })?
            .copy_from_slice(bytes);
        Ok(())
    }

    fn position(&mut self) -> IonResult<u64> {
        Ok(self.len() as u64)
    }
}
