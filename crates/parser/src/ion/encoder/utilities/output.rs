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

    pub fn sweep_orphans(output_path: &Path) {
        let output_folder = output_path.parent().unwrap_or_else(|| Path::new("."));
        let output_name = match output_path.file_name().and_then(|value| value.to_str()) {
            Some(name) => name,
            None => return,
        };
        let temp_prefix = format!(".{output_name}.tmp.");
        let files = match fs::read_dir(output_folder) {
            Ok(files) => files,
            Err(_) => return,
        };
        let current_pid = std::process::id();
        for file in files.flatten() {
            let file_name = file.file_name();
            let file_name = match file_name.to_str() {
                Some(name) => name,
                None => continue,
            };
            let pid_and_time = match file_name.strip_prefix(&temp_prefix) {
                Some(suffix) => suffix,
                None => continue,
            };
            let owner_pid = match pid_and_time
                .split('.')
                .next()
                .and_then(|part| part.parse::<u32>().ok())
            {
                Some(pid) => pid,
                None => continue,
            };
            if owner_pid != current_pid && !process_is_running(owner_pid) {
                let _ = fs::remove_file(file.path());
            }
        }
    }
}

#[cfg(unix)]
fn process_is_running(pid: u32) -> bool {
    unsafe extern "C" {
        fn kill(pid: i32, signal: i32) -> i32;
    }
    if unsafe { kill(pid as i32, 0) } == 0 {
        return true;
    }
    std::io::Error::last_os_error().kind() == std::io::ErrorKind::PermissionDenied
}

#[cfg(windows)]
fn process_is_running(pid: u32) -> bool {
    use std::ffi::c_void;

    const SYNCHRONIZE: u32 = 0x0010_0000;
    const WAIT_TIMEOUT: u32 = 0x0000_0102;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn OpenProcess(access: u32, inherit_handle: i32, process_id: u32) -> *mut c_void;
        fn WaitForSingleObject(handle: *mut c_void, milliseconds: u32) -> u32;
        fn CloseHandle(handle: *mut c_void) -> i32;
    }

    let handle = unsafe { OpenProcess(SYNCHRONIZE, 0, pid) };
    if handle.is_null() {
        return std::io::Error::last_os_error().kind() == std::io::ErrorKind::PermissionDenied;
    }
    let state = unsafe { WaitForSingleObject(handle, 0) };
    unsafe {
        CloseHandle(handle);
    }
    state == WAIT_TIMEOUT
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

#[cfg(all(test, not(all(target_arch = "wasm32", not(target_os = "wasi")))))]
mod tests {
    use std::process::{Child, Command};

    use super::*;

    fn make_test_folder() -> PathBuf {
        let process_id = std::process::id();
        let time_id = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|time| time.as_nanos())
            .unwrap_or(0);
        let folder = std::env::temp_dir().join(format!("ionic-sweep-test.{process_id}.{time_id}"));
        fs::create_dir_all(&folder).unwrap();
        folder
    }

    #[cfg(unix)]
    fn spawn_blocking_process() -> Child {
        Command::new("sleep").arg("30").spawn().unwrap()
    }

    #[cfg(windows)]
    fn spawn_blocking_process() -> Child {
        Command::new("ping")
            .args(["-n", "31", "127.0.0.1"])
            .stdout(std::process::Stdio::null())
            .spawn()
            .unwrap()
    }

    #[cfg(unix)]
    fn dead_process_id() -> u32 {
        let mut child = Command::new("true").spawn().unwrap();
        let pid = child.id();
        child.wait().unwrap();
        pid
    }

    #[cfg(windows)]
    fn dead_process_id() -> u32 {
        let mut child = Command::new("cmd").args(["/C", "exit"]).spawn().unwrap();
        let pid = child.id();
        child.wait().unwrap();
        pid
    }

    #[test]
    fn process_is_running_reports_current_process_as_alive() {
        assert!(process_is_running(std::process::id()));
    }

    #[test]
    fn process_is_running_reports_dead_process() {
        assert!(!process_is_running(dead_process_id()));
    }

    #[test]
    fn sweep_removes_only_dead_owner_temps() {
        let folder = make_test_folder();
        let output_path = folder.join("SA1.ion");

        let mut live_child = spawn_blocking_process();
        let live_pid = live_child.id();

        let live_temp = folder.join(format!(".SA1.ion.tmp.{live_pid}.111"));
        let dead_temp = folder.join(format!(".SA1.ion.tmp.{}.222", dead_process_id()));
        let other_output_temp = folder.join(format!(".SB1.ion.tmp.{}.333", dead_process_id()));

        fs::write(&live_temp, b"x").unwrap();
        fs::write(&dead_temp, b"x").unwrap();
        fs::write(&other_output_temp, b"x").unwrap();

        TempFile::sweep_orphans(&output_path);

        assert!(live_temp.exists());
        assert!(!dead_temp.exists());
        assert!(other_output_temp.exists());

        live_child.kill().ok();
        live_child.wait().ok();
        fs::remove_dir_all(&folder).unwrap();
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
            .ok_or_else(|| IonError::from(format!("patch: range {start}..{end} out of bounds")))?
            .copy_from_slice(bytes);
        Ok(())
    }

    fn position(&mut self) -> IonResult<u64> {
        Ok(self.len() as u64)
    }
}
