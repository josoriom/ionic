pub(crate) mod container_builder;
pub(crate) use container_builder::{CompressionMode, ContainerBuilder, DefaultCompressor};
pub(crate) mod encoder_output;
pub use encoder_output::{EncoderOutput, SectionChunkMode};
#[cfg(not(all(target_arch = "wasm32", not(target_os = "wasi"))))]
pub use encoder_output::{FileEncoderOutput, TempFile};
pub(crate) use encoder_output::{SectionChunk, make_chunk};
pub(crate) mod file_header_writer;
pub(crate) use file_header_writer::FileHeader;
pub(crate) mod le_writers;
pub(crate) mod meta_collector;
pub(crate) mod segments;
pub(crate) mod tables;
