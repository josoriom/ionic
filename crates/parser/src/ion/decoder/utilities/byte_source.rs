use std::sync::Arc;

use crate::ion::{ByteRange, IonError, IonResult};

pub trait ReadBytes: Send + Sync {
    fn read(&self, range: ByteRange) -> IonResult<SourceBytes>;
}

#[derive(Clone)]
pub enum SourceBytes {
    Owned(Vec<u8>),
    Memory { data: Arc<[u8]>, start: usize, end: usize },
    #[cfg(not(all(target_arch = "wasm32", not(target_os = "wasi"))))]
    Mapped { map: Arc<memmap2::Mmap>, start: usize, end: usize },
}

impl std::ops::Deref for SourceBytes {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        match self {
            SourceBytes::Owned(bytes) => bytes,
            SourceBytes::Memory { data, start, end } => &data[*start..*end],
            #[cfg(not(all(target_arch = "wasm32", not(target_os = "wasi"))))]
            SourceBytes::Mapped { map, start, end } => &map[*start..*end],
        }
    }
}

impl SourceBytes {
    pub fn into_arc(self) -> Arc<[u8]> {
        match self {
            SourceBytes::Owned(bytes) => Arc::from(bytes),
            SourceBytes::Memory { data, start, end } => Arc::from(&data[start..end]),
            #[cfg(not(all(target_arch = "wasm32", not(target_os = "wasi"))))]
            SourceBytes::Mapped { map, start, end } => Arc::from(&map[start..end]),
        }
    }
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
    fn read(&self, range: ByteRange) -> IonResult<SourceBytes> {
        let start = to_usize(range.offset, "offset")?;
        let end = start
            .checked_add(to_usize(range.length, "length")?)
            .ok_or_else(|| IonError::from("byte source: range overflows"))?;
        if self.data.get(start..end).is_some() {
            Ok(SourceBytes::Memory {
                data: self.data.clone(),
                start,
                end,
            })
        } else {
            Err(IonError::from("byte source: read out of bounds"))
        }
    }
}

#[cfg(not(all(target_arch = "wasm32", not(target_os = "wasi"))))]
pub struct FileSource {
    map: Arc<memmap2::Mmap>,
}

#[cfg(not(all(target_arch = "wasm32", not(target_os = "wasi"))))]
impl FileSource {
    pub fn new(map: memmap2::Mmap) -> Self {
        Self { map: Arc::new(map) }
    }
}

#[cfg(not(all(target_arch = "wasm32", not(target_os = "wasi"))))]
impl ReadBytes for FileSource {
    fn read(&self, range: ByteRange) -> IonResult<SourceBytes> {
        let start = to_usize(range.offset, "offset")?;
        let end = start
            .checked_add(to_usize(range.length, "length")?)
            .ok_or_else(|| IonError::from("byte source: range overflows"))?;
        if self.map.get(start..end).is_some() {
            Ok(SourceBytes::Mapped {
                map: self.map.clone(),
                start,
                end,
            })
        } else {
            Err(IonError::from("byte source: read out of bounds"))
        }
    }
}

pub type RangeReader = dyn Fn(ByteRange) -> IonResult<Vec<u8>> + Send + Sync;

pub struct CallbackSource {
    read: Box<RangeReader>,
}

impl CallbackSource {
    pub fn new(read: impl Fn(ByteRange) -> IonResult<Vec<u8>> + Send + Sync + 'static) -> Self {
        Self {
            read: Box::new(read),
        }
    }
}

impl ReadBytes for CallbackSource {
    fn read(&self, range: ByteRange) -> IonResult<SourceBytes> {
        let bytes = (self.read)(range)?;
        allow_len(&bytes, range.length)?;
        Ok(SourceBytes::Owned(bytes))
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
