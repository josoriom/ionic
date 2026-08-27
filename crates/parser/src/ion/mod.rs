pub(crate) mod array_kind;
pub(crate) mod byte_transpose;
pub mod filter_summary;
pub(crate) mod header;
pub(crate) mod meta_groups;
pub(crate) mod packing;
pub mod range;
pub(crate) mod windowing;
pub use array_kind::ArrayKind;
pub use filter_summary::{ChromatogramSummary, SpectrumSummary};
pub use range::{ByteRange, Range};
pub(crate) mod decoder;
pub(crate) mod format;
pub mod scan;
pub(crate) mod version_generated;
pub(crate) use decoder::utilities;
#[cfg(not(all(target_arch = "wasm32", not(target_os = "wasi"))))]
pub use decoder::utilities::byte_source::FileSource;
pub use decoder::{
    decode::{
        ArrayAddress, DataXY, IonReader, ItemKind, ItemSlice, Pixel, Query, ReadOptions, Select,
        Window, coalesce_byte_ranges, open_ranges,
    },
    utilities::{
        byte_source::{BytesSource, CallbackSource, ReadBytes, SourceBytes},
        decompression_limit::{DEFAULT_MAX_UNCOMPRESSED_SIZE, DecompressionLimit},
    },
};
pub use header::{
    HEADER_FORMAT_VERSION_OFFSET, get_total_file_size_from_header, get_version_from_header,
};
pub(crate) mod encoder;
#[cfg(not(all(target_arch = "wasm32", not(target_os = "wasi"))))]
pub use encoder::utilities::FileWriter;
pub use encoder::{
    encode::{DEFAULT_MZ_WINDOW, TARGET_BLOCK_UNCOMPRESSED_BYTES, WriteOptions},
    ion_writer::{IonWriter, write_mzml_to_ion},
    scan_stream::{MemoryReader, ScanStream},
    utilities::{SectionStorage, WriteBytes},
};
pub use format::{
    CODEC_NONE, CODEC_ZSTD, CURRENT_VERSION, FILE_SIGNATURE, FILE_TRAILER, HEADER_SIZE,
    MAX_SUPPORTED_VERSION, MIN_SUPPORTED_VERSION, allow_version, is_supported,
};
pub(crate) mod attr_meta;
pub mod error;
pub use error::{IonError, IonResult};
