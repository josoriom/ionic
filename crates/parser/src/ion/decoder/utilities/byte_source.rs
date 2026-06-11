use std::sync::Arc;

use crate::ion::{IonError, IonResult, Range};

pub trait ReadBytes: Send + Sync {
    fn read(&self, range: Range) -> IonResult<Vec<u8>>;
}

pub struct BytesSource {
    data: Arc<[u8]>,
}

impl BytesSource {
    pub fn new(data: Arc<[u8]>) -> Self {
        Self { data }
    }
}

impl ReadBytes for BytesSource {
    fn read(&self, range: Range) -> IonResult<Vec<u8>> {
        let start = to_usize(range.offset, "offset")?;
        let end = start
            .checked_add(to_usize(range.length, "length")?)
            .ok_or_else(|| IonError::from("byte source: range overflows"))?;
        self.data
            .get(start..end)
            .map(|bytes| bytes.to_vec())
            .ok_or_else(|| IonError::from("byte source: read out of bounds"))
    }
}

#[cfg(not(all(target_arch = "wasm32", not(target_os = "wasi"))))]
pub struct FileSource {
    map: memmap2::Mmap,
}

#[cfg(not(all(target_arch = "wasm32", not(target_os = "wasi"))))]
impl FileSource {
    pub fn new(map: memmap2::Mmap) -> Self {
        Self { map }
    }
}

#[cfg(not(all(target_arch = "wasm32", not(target_os = "wasi"))))]
impl ReadBytes for FileSource {
    fn read(&self, range: Range) -> IonResult<Vec<u8>> {
        let start = to_usize(range.offset, "offset")?;
        let end = start
            .checked_add(to_usize(range.length, "length")?)
            .ok_or_else(|| IonError::from("byte source: range overflows"))?;
        self.map
            .get(start..end)
            .map(|bytes| bytes.to_vec())
            .ok_or_else(|| IonError::from("byte source: read out of bounds"))
    }
}

pub type RangeReader = dyn Fn(Range) -> IonResult<Vec<u8>> + Send + Sync;

pub struct CallbackSource {
    read: Box<RangeReader>,
}

impl CallbackSource {
    pub fn new(read: impl Fn(Range) -> IonResult<Vec<u8>> + Send + Sync + 'static) -> Self {
        Self {
            read: Box::new(read),
        }
    }
}

impl ReadBytes for CallbackSource {
    fn read(&self, range: Range) -> IonResult<Vec<u8>> {
        let bytes = (self.read)(range)?;
        allow_len(&bytes, range.length)?;
        Ok(bytes)
    }
}

fn allow_len(bytes: &[u8], length: u64) -> IonResult<()> {
    let expected_len = to_usize(length, "length")?;
    if bytes.len() != expected_len {
        return Err(IonError::from(format!(
            "byte source: expected {expected_len} bytes, got {}",
            bytes.len()
        )));
    }
    Ok(())
}

fn to_usize(value: u64, name: &str) -> IonResult<usize> {
    usize::try_from(value).map_err(|_| IonError::from(format!("byte source: {name} out of range")))
}
