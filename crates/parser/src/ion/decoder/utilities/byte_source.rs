use std::{future::Future, pin::Pin, sync::Arc};

use crate::ion::{IonError, IonResult};

pub trait ByteSource: Send + Sync {
    fn read(&self, offset: u64, length: u64) -> IonResult<Vec<u8>>;
}

pub trait AsyncByteSource {
    fn read(&self, query: Query) -> QueryFuture<'_>;
}

pub type QueryFuture<'a> = Pin<Box<dyn Future<Output = IonResult<QueryValue>> + 'a>>;

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

impl AsyncByteSource for SliceSource {
    fn read(&self, query: Query) -> QueryFuture<'_> {
        Box::pin(async move {
            let bytes = ByteSource::read(self, query.offset(), query.length())?;
            Ok(QueryValue::new(bytes))
        })
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

#[cfg(not(all(target_arch = "wasm32", not(target_os = "wasi"))))]
impl AsyncByteSource for MmapSource {
    fn read(&self, query: Query) -> QueryFuture<'_> {
        Box::pin(async move {
            let bytes = ByteSource::read(self, query.offset(), query.length())?;
            Ok(QueryValue::new(bytes))
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Query {
    offset: u64,
    length: u64,
}

impl Query {
    pub fn new(offset: u64, length: u64) -> Self {
        Self { offset, length }
    }

    pub fn offset(&self) -> u64 {
        self.offset
    }

    pub fn length(&self) -> u64 {
        self.length
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryValue {
    bytes: Vec<u8>,
}

impl QueryValue {
    pub fn new(bytes: Vec<u8>) -> Self {
        Self { bytes }
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }
}

impl From<Vec<u8>> for QueryValue {
    fn from(bytes: Vec<u8>) -> Self {
        Self::new(bytes)
    }
}

pub type QueryReader = dyn Fn(Query) -> IonResult<QueryValue> + Send + Sync;

pub struct QueryCallbackSource {
    read: Box<QueryReader>,
}

impl QueryCallbackSource {
    pub fn new(read: impl Fn(Query) -> IonResult<QueryValue> + Send + Sync + 'static) -> Self {
        Self {
            read: Box::new(read),
        }
    }
}

impl ByteSource for QueryCallbackSource {
    fn read(&self, offset: u64, length: u64) -> IonResult<Vec<u8>> {
        let query = Query::new(offset, length);
        let value = (self.read)(query)?;
        let bytes = value.into_bytes();
        allow_len(&bytes, length)?;
        Ok(bytes)
    }
}

impl AsyncByteSource for QueryCallbackSource {
    fn read(&self, query: Query) -> QueryFuture<'_> {
        Box::pin(async move {
            let bytes = ByteSource::read(self, query.offset(), query.length())?;
            Ok(QueryValue::new(bytes))
        })
    }
}

pub type AsyncQueryReader = dyn Fn(Query) -> QueryFuture<'static>;

pub struct AsyncQueryCallbackSource {
    read: Box<AsyncQueryReader>,
}

impl AsyncQueryCallbackSource {
    pub fn new(read: impl Fn(Query) -> QueryFuture<'static> + 'static) -> Self {
        Self {
            read: Box::new(read),
        }
    }
}

impl AsyncByteSource for AsyncQueryCallbackSource {
    fn read(&self, query: Query) -> QueryFuture<'_> {
        Box::pin(async move {
            let length = query.length();
            let value = (self.read)(query).await?;
            let bytes = value.bytes();
            allow_len(bytes, length)?;
            Ok(value)
        })
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
