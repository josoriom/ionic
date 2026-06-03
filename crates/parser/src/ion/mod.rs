pub(crate) mod byte_transpose;
pub mod filter_summary;
pub(crate) mod meta_groups;
pub(crate) mod packing;
pub use filter_summary::{ChromatogramSummary, SpectrumSummary};
pub mod decoder;
pub mod format;
pub(crate) mod version_generated;
pub use decoder::decode::{Decoder, DecoderConfig, Ion, OwnedIon};
pub(crate) use decoder::utilities;
pub use decoder::utilities::decompression_budget::{
    DEFAULT_MAX_UNCOMPRESSED_SIZE, DecompressionBudget,
};
pub use decoder::utilities::parse_header::{HEADER_FORMAT_VERSION_OFFSET, get_version_from_header};
pub mod encoder;
pub use encoder::{encode::encode, utilities::FileEncoderOutput};
pub mod attr_meta;
pub mod error;
pub use error::{IonError, IonResult};
