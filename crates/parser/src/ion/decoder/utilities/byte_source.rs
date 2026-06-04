use std::sync::Arc;

use crate::ion::{IonError, IonResult};

pub trait ByteSource: Send + Sync {
    fn read(&self, offset: u64, length: u64) -> IonResult<Vec<u8>>;
}

pub struct SliceSource {
    data: Arc<[u8]>,
}

impl SliceSource {
    pub fn new(data: Arc<[u8]>) -> Self {
        Self { data }
    }
}

impl ByteSource for SliceSource {
    fn read(&self, offset: u64, length: u64) -> IonResult<Vec<u8>> {
        let start = to_usize(offset, "offset")?;
        let end = start
            .checked_add(to_usize(length, "length")?)
            .ok_or_else(|| IonError::from("byte source: range overflows"))?;
        self.data
            .get(start..end)
            .map(|bytes| bytes.to_vec())
            .ok_or_else(|| IonError::from("byte source: read out of bounds"))
    }
}

#[cfg(not(all(target_arch = "wasm32", not(target_os = "wasi"))))]
pub struct MmapSource {
    map: memmap2::Mmap,
}

#[cfg(not(all(target_arch = "wasm32", not(target_os = "wasi"))))]
impl MmapSource {
    pub fn new(map: memmap2::Mmap) -> Self {
        Self { map }
    }
}

#[cfg(not(all(target_arch = "wasm32", not(target_os = "wasi"))))]
impl ByteSource for MmapSource {
    fn read(&self, offset: u64, length: u64) -> IonResult<Vec<u8>> {
        let start = to_usize(offset, "offset")?;
        let end = start
            .checked_add(to_usize(length, "length")?)
            .ok_or_else(|| IonError::from("byte source: range overflows"))?;
        self.map
            .get(start..end)
            .map(|bytes| bytes.to_vec())
            .ok_or_else(|| IonError::from("byte source: read out of bounds"))
    }
}

fn to_usize(value: u64, name: &str) -> IonResult<usize> {
    usize::try_from(value)
        .map_err(|_| IonError::from(format!("byte source: {name} out of range")))
}
