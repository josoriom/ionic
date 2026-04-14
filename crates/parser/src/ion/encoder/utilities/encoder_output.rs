use std::fs::File;
use std::io::{BufWriter, Seek, SeekFrom, Write};

use crate::ion::{IonError, IonResult};

pub trait EncoderOutput {
    fn write_bytes(&mut self, bytes: &[u8]) -> IonResult<()>;
    fn patch_bytes_at(&mut self, position: u64, bytes: &[u8]) -> IonResult<()>;
    fn current_byte_position(&mut self) -> IonResult<u64>;
}

pub struct FileEncoderOutput {
    writer: BufWriter<File>,
}

impl FileEncoderOutput {
    pub fn open_for_writing(path: &str) -> IonResult<Self> {
        let file = File::create(path)
            .map_err(|err| IonError::from(format!("cannot create output file '{path}': {err}")))?;
        Ok(Self {
            writer: BufWriter::with_capacity(8 * 1024 * 1024, file),
        })
    }
}

impl EncoderOutput for FileEncoderOutput {
    fn write_bytes(&mut self, bytes: &[u8]) -> IonResult<()> {
        self.writer
            .write_all(bytes)
            .map_err(|err| IonError::from(format!("write error: {err}")))
    }

    fn patch_bytes_at(&mut self, position: u64, bytes: &[u8]) -> IonResult<()> {
        self.writer
            .flush()
            .map_err(|err| IonError::from(format!("flush error: {err}")))?;
        let resume_position = self
            .writer
            .stream_position()
            .map_err(|err| IonError::from(format!("position error: {err}")))?;
        self.writer
            .seek(SeekFrom::Start(position))
            .map_err(|err| IonError::from(format!("seek error: {err}")))?;
        self.writer
            .write_all(bytes)
            .map_err(|err| IonError::from(format!("patch write error: {err}")))?;
        self.writer
            .seek(SeekFrom::Start(resume_position))
            .map_err(|err| IonError::from(format!("seek-resume error: {err}")))?;
        Ok(())
    }

    fn current_byte_position(&mut self) -> IonResult<u64> {
        self.writer
            .flush()
            .map_err(|err| IonError::from(format!("flush error: {err}")))?;
        self.writer
            .stream_position()
            .map_err(|err| IonError::from(format!("position error: {err}")))
    }
}

impl EncoderOutput for Vec<u8> {
    fn write_bytes(&mut self, bytes: &[u8]) -> IonResult<()> {
        self.extend_from_slice(bytes);
        Ok(())
    }

    fn patch_bytes_at(&mut self, position: u64, bytes: &[u8]) -> IonResult<()> {
        let start = position as usize;
        let end = start + bytes.len();
        self.get_mut(start..end)
            .ok_or_else(|| {
                IonError::from(format!(
                    "patch_bytes_at: range {start}..{end} out of bounds"
                ))
            })?
            .copy_from_slice(bytes);
        Ok(())
    }

    fn current_byte_position(&mut self) -> IonResult<u64> {
        Ok(self.len() as u64)
    }
}
