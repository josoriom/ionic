pub(crate) mod block_writer;
pub(crate) use block_writer::{BlockWriter, CompressionMode, DefaultCompressor};
pub(crate) mod output;
#[cfg(not(all(target_arch = "wasm32", not(target_os = "wasi"))))]
pub use output::FileWriter;
#[cfg(not(all(target_arch = "wasm32", not(target_os = "wasi"))))]
pub(crate) use output::TempFile;
pub(crate) use output::{SectionChunk, make_chunk};
pub use output::{SectionStorage, WriteBytes};
pub(crate) mod le_writers;
pub(crate) mod meta_collector;
pub(crate) mod tables;
