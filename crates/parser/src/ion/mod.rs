pub(crate) mod axes;
pub(crate) mod byte_transpose;
pub(crate) mod extensions;
pub mod filter_summary;
pub(crate) mod meta_groups;
pub(crate) mod packing;
pub use filter_summary::{ChromatogramSummary, SpectrumSummary};
pub mod decoder;
pub mod format;
pub(crate) mod version_generated;
pub use decoder::decode::{ArrayWindow, Decoder, DecoderConfig, Ion, OwnedIon, SpectrumWindow};
pub(crate) use decoder::utilities;
#[cfg(not(all(target_arch = "wasm32", not(target_os = "wasi"))))]
pub use decoder::utilities::byte_source::MmapSource;
pub use decoder::utilities::byte_source::{
    AsyncByteSource, AsyncQueryCallbackSource, AsyncQueryReader, ByteSource, Query,
    QueryCallbackSource, QueryPromise, QueryReader, QueryPayload, SliceSource,
};
pub use decoder::utilities::decompression_budget::{
    DEFAULT_MAX_UNCOMPRESSED_SIZE, DecompressionBudget,
};
pub use decoder::utilities::parse_header::{HEADER_FORMAT_VERSION_OFFSET, get_version_from_header};
pub mod encoder;
pub use encoder::encode::encode;
pub use encoder::utilities::EncoderOutput;
#[cfg(not(all(target_arch = "wasm32", not(target_os = "wasi"))))]
pub use encoder::utilities::{FileEncoderOutput, TempFile};
pub mod attr_meta;
pub mod error;
pub use error::{IonError, IonResult};
