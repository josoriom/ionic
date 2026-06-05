pub(crate) mod accessions;
pub mod ion;
pub mod mzml;
pub mod utilities;

pub use ion::{
    AsyncByteSource, AsyncQueryCallbackSource, AsyncQueryReader, ByteSource,
    ChromatogramSummary, Query, QueryCallbackSource, QueryFuture, QueryReader, QueryPayload,
    SliceSource, SpectrumSummary, decoder,
    decoder::decode::{Metadatum, MetadatumValue},
    decoder::utilities::spectrum_source::{ScanSource, ScanSummary},
    encoder,
    encoder::encode::EncodingConfig,
    encoder::file_reader::{FileReader, MemoryReader},
    encoder::ion_writer::{IonWriter, stream_to_ion, write_mzml_to_ion},
    encoder::utilities::SectionChunkMode,
};

#[cfg(not(all(target_arch = "wasm32", not(target_os = "wasi"))))]
pub use ion::MmapSource;
pub use mzml::{BinToMzmlError, bin_to_mzml, parse_indexed_mzml, parse_mzml};
