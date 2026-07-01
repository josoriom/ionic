pub(crate) mod byte_transpose;
pub mod filter_summary;
pub(crate) mod header;
pub(crate) mod meta_groups;
pub(crate) mod packing;
pub mod range;
pub(crate) mod windowing;
pub use filter_summary::{ChromatogramSummary, SpectrumSummary};
pub use range::{ByteRange, Range};
pub mod decoder;
pub mod format;
pub(crate) mod version_generated;
pub use decoder::decode::{
    DataXY, IonReader, ItemSlice, Pixel, ReadOptions, Select, Target, Window, open_ranges,
};
pub(crate) use decoder::utilities;
#[cfg(not(all(target_arch = "wasm32", not(target_os = "wasi"))))]
pub use decoder::utilities::byte_source::FileSource;
pub use decoder::utilities::byte_source::{
    BytesSource, CallbackSource, ReadBytes,
};
pub use decoder::utilities::decompression_limit::{
    DEFAULT_MAX_UNCOMPRESSED_SIZE, DecompressionLimit,
};
pub use header::{HEADER_FORMAT_VERSION_OFFSET, get_version_from_header};
pub mod encoder;
pub use encoder::encode::{encode, WriteOptions};
pub use encoder::utilities::{WriteBytes, SectionStorage};
#[cfg(not(all(target_arch = "wasm32", not(target_os = "wasi"))))]
pub use encoder::utilities::{FileWriter, TempFile};
pub mod attr_meta;
pub mod error;
pub use error::{IonError, IonResult};
