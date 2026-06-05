use std::sync::Arc;

use crate::ion::{
    IonResult,
    decoder::decode::{ArrayRef, Decoder, DecoderConfig, open_byte_ranges},
    decoder::utilities::byte_source::{
        AsyncByteSource, AsyncQueryCallbackSource, ByteSource, CacheBackedSource, Query,
        QueryPromise, read_all,
    },
    decoder::utilities::parse_header::parse_header,
    decoder::utilities::spectrum_source::{ScanSource, ScanSummary},
};
use crate::mzml::structs::{MzML, Spectrum};

pub(crate) struct AsyncReader {
    source: Arc<dyn AsyncByteSource>,
    cache: Arc<CacheBackedSource>,
    decoder: Decoder,
}

impl AsyncReader {
    pub async fn open_with_async_query(
        read: impl Fn(Query) -> QueryPromise<'static> + 'static,
        config: DecoderConfig,
    ) -> IonResult<Self> {
        let source = Arc::new(AsyncQueryCallbackSource::new(read)) as Arc<dyn AsyncByteSource>;
        Self::open_with_async_source(source, config).await
    }

    pub async fn open_with_async_source(
        source: Arc<dyn AsyncByteSource>,
        config: DecoderConfig,
    ) -> IonResult<Self> {
        let cache = Arc::new(CacheBackedSource::new());

        let header_bytes = source.read(Query::new(0, 1024)).await?.into_bytes();
        let header = parse_header(&header_bytes)?;
        cache.fill(0, 1024, header_bytes);

        let ranges = open_byte_ranges(&header)?;
        fetch_missing(source.as_ref(), cache.as_ref(), &ranges).await?;

        let byte_source = cache.clone() as Arc<dyn ByteSource>;
        let decoder = Decoder::open_with_source(byte_source, config)?;

        Ok(Self {
            source,
            cache,
            decoder,
        })
    }

    pub fn decoder(&self) -> &Decoder {
        &self.decoder
    }

    pub fn decoder_mut(&mut self) -> &mut Decoder {
        &mut self.decoder
    }

    async fn fetch_ranges(&self, ranges: &[(u64, u64)]) -> IonResult<()> {
        fetch_missing(self.source.as_ref(), self.cache.as_ref(), ranges).await
    }

    pub async fn to_mzml(&mut self) -> IonResult<MzML> {
        let ranges = self.decoder.mzml_block_ranges();
        self.fetch_ranges(&ranges).await?;
        self.decoder.to_mzml()
    }

    pub async fn spectrum_at(&mut self, index: usize) -> IonResult<Option<Spectrum>> {
        let ranges = self.decoder.spectrum_block_ranges(index);
        self.fetch_ranges(&ranges).await?;
        self.decoder.spectrum_at(index)
    }

    pub async fn read_spectrum_array(
        &mut self,
        aref: &ArrayRef,
        out: &mut Vec<f64>,
    ) -> IonResult<()> {
        if let Some(range) = self.decoder.spec_block_range(aref.block_id) {
            self.fetch_ranges(&[range]).await?;
        }
        self.decoder.read_spectrum_array(aref, out)
    }

    pub async fn read_chromatogram_array(
        &mut self,
        aref: &ArrayRef,
        out: &mut Vec<f64>,
    ) -> IonResult<()> {
        if let Some(range) = self.decoder.chrom_block_range(aref.block_id) {
            self.fetch_ranges(&[range]).await?;
        }
        self.decoder.read_chromatogram_array(aref, out)
    }

    pub async fn load_scan(
        &mut self,
        index: usize,
        mz: &mut Vec<f64>,
        intensity: &mut Vec<f64>,
    ) -> IonResult<bool> {
        let ranges = self.decoder.spectrum_block_ranges(index);
        self.fetch_ranges(&ranges).await?;
        Ok(self.decoder.load_scan(index, mz, intensity))
    }

    pub async fn for_each_in_range<F>(
        &mut self,
        rt_min: f64,
        rt_max: f64,
        ms_level: u8,
        callback: F,
    ) -> IonResult<()>
    where
        F: FnMut(&ScanSummary, &[f64], &[f64]),
    {
        let ranges = self.decoder.spec_block_ranges_all();
        self.fetch_ranges(&ranges).await?;
        self.decoder
            .for_each_in_range(rt_min, rt_max, ms_level, callback);
        Ok(())
    }
}

async fn fetch_missing(
    source: &dyn AsyncByteSource,
    cache: &CacheBackedSource,
    ranges: &[(u64, u64)],
) -> IonResult<()> {
    let missing = cache.missing(ranges);
    if missing.is_empty() {
        return Ok(());
    }
    let payloads = read_all(source, &missing).await?;
    for (&(offset, length), payload) in missing.iter().zip(payloads) {
        cache.fill(offset, length, payload.into_bytes());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ion::IonError;
    use crate::ion::decoder::utilities::byte_source::QueryPayload;
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

    const BYTES: &[u8] = include_bytes!("../../../data/ion/test.ion");

    fn block_on<F: Future>(future: F) -> F::Output {
        fn noop(_: *const ()) {}
        fn clone(_: *const ()) -> RawWaker {
            RawWaker::new(std::ptr::null(), &VTABLE)
        }
        static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, noop, noop, noop);
        let waker = unsafe { Waker::from_raw(RawWaker::new(std::ptr::null(), &VTABLE)) };
        let mut context = Context::from_waker(&waker);
        let mut future = Box::pin(future);
        loop {
            if let Poll::Ready(value) = future.as_mut().poll(&mut context) {
                return value;
            }
        }
    }

    struct YieldOnce {
        done: bool,
    }

    impl Future for YieldOnce {
        type Output = ();
        fn poll(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<()> {
            if self.done {
                return Poll::Ready(());
            }
            self.done = true;
            context.waker().wake_by_ref();
            Poll::Pending
        }
    }

    struct RangeServer {
        data: Arc<[u8]>,
        reads: AtomicUsize,
    }

    impl RangeServer {
        fn new(bytes: &[u8]) -> Arc<Self> {
            Arc::new(Self {
                data: Arc::from(bytes),
                reads: AtomicUsize::new(0),
            })
        }
    }

    impl AsyncByteSource for RangeServer {
        fn read(&self, query: Query) -> QueryPromise<'_> {
            self.reads.fetch_add(1, Ordering::Relaxed);
            let data = self.data.clone();
            Box::pin(async move {
                YieldOnce { done: false }.await;
                let start = query.offset() as usize;
                let end = start + query.length() as usize;
                if end > data.len() {
                    return Err(IonError::from("range server: out of bounds"));
                }
                Ok(QueryPayload::new(data[start..end].to_vec()))
            })
        }
    }

    fn open_async(server: Arc<RangeServer>) -> AsyncReader {
        let source = server as Arc<dyn AsyncByteSource>;
        block_on(AsyncReader::open_with_async_source(
            source,
            DecoderConfig::default(),
        ))
        .unwrap()
    }

    struct CountingServer {
        data: Arc<[u8]>,
        in_flight: AtomicUsize,
        peak_in_flight: AtomicUsize,
    }

    impl CountingServer {
        fn new(bytes: &[u8]) -> Arc<Self> {
            Arc::new(Self {
                data: Arc::from(bytes),
                in_flight: AtomicUsize::new(0),
                peak_in_flight: AtomicUsize::new(0),
            })
        }
    }

    impl AsyncByteSource for CountingServer {
        fn read(&self, query: Query) -> QueryPromise<'_> {
            let data = self.data.clone();
            Box::pin(async move {
                let started = self.in_flight.fetch_add(1, Ordering::Relaxed) + 1;
                self.peak_in_flight.fetch_max(started, Ordering::Relaxed);
                YieldOnce { done: false }.await;
                self.in_flight.fetch_sub(1, Ordering::Relaxed);
                let start = query.offset() as usize;
                let end = start + query.length() as usize;
                if end > data.len() {
                    return Err(IonError::from("counting server: out of bounds"));
                }
                Ok(QueryPayload::new(data[start..end].to_vec()))
            })
        }
    }

    #[test]
    fn async_open_fetches_ranges_concurrently() {
        let server = CountingServer::new(BYTES);
        let source = server.clone() as Arc<dyn AsyncByteSource>;
        let reader = block_on(AsyncReader::open_with_async_source(
            source,
            DecoderConfig::default(),
        ))
        .unwrap();
        assert!(server.peak_in_flight.load(Ordering::Relaxed) > 1);
        drop(reader);
    }

    #[test]
    fn async_to_mzml_matches_sync() {
        let mut sync = Decoder::open(BYTES, DecoderConfig::default()).unwrap();
        let mut reader = open_async(RangeServer::new(BYTES));

        let from_sync = sync.to_mzml().unwrap();
        let from_async = block_on(reader.to_mzml()).unwrap();

        assert_eq!(format!("{from_sync:?}"), format!("{from_async:?}"));
    }

    #[test]
    fn async_spectrum_at_matches_sync() {
        let mut sync = Decoder::open(BYTES, DecoderConfig::default()).unwrap();
        let mut reader = open_async(RangeServer::new(BYTES));

        for index in 0..sync.spectrum_count() as usize {
            let from_sync = sync.spectrum_at(index).unwrap();
            let from_async = block_on(reader.spectrum_at(index)).unwrap();
            assert_eq!(format!("{from_sync:?}"), format!("{from_async:?}"));
        }
    }

    #[test]
    fn async_read_spectrum_array_matches_sync() {
        let mut sync = Decoder::open(BYTES, DecoderConfig::default()).unwrap();
        let mut reader = open_async(RangeServer::new(BYTES));

        let refs = sync.spectrum_array_refs(0).unwrap();
        for aref in &refs {
            let mut want = Vec::new();
            sync.read_spectrum_array(aref, &mut want).unwrap();
            let mut got = Vec::new();
            block_on(reader.read_spectrum_array(aref, &mut got)).unwrap();
            assert_eq!(want, got);
        }
    }

    #[test]
    fn async_open_with_zero_cache() {
        let mut config = DecoderConfig::default();
        config.max_cached_bytes = 0;
        let source = RangeServer::new(BYTES) as Arc<dyn AsyncByteSource>;
        let mut reader =
            block_on(AsyncReader::open_with_async_source(source, config)).unwrap();
        let mzml = block_on(reader.to_mzml()).unwrap();
        assert!(mzml.run.spectrum_list.unwrap().spectra.len() > 0);
    }

    #[test]
    fn async_reader_suspends_at_least_once() {
        let server = RangeServer::new(BYTES);
        let reader = open_async(server.clone());
        assert!(server.reads.load(Ordering::Relaxed) > 0);
        drop(reader);
    }

    #[test]
    fn cache_backed_source_reports_prefetch_miss() {
        let cache = CacheBackedSource::new();
        cache.fill(0, 4, vec![1, 2, 3, 4]);
        assert_eq!(cache.read(0, 4).unwrap(), vec![1, 2, 3, 4]);
        let missing = cache.read(8, 16);
        assert!(missing.is_err());
        assert!(
            missing
                .unwrap_err()
                .to_string()
                .contains("prefetch miss")
        );
    }
}
