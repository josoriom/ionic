pub(crate) mod accessions;
pub mod ion;
pub mod mzml;
pub mod utilities;

pub use ion::{
    BytesSource, CallbackSource, ChromatogramSummary, ReadBytes, Range,
    SpectrumSummary, decoder,
    decoder::decode::{Metadatum, MetadatumValue},
    decoder::utilities::spectrum_source::{ScanSource, ScanSummary},
    encoder,
    encoder::encode::WriteOptions,
    encoder::scan_stream::{ScanStream, MemoryReader},
    encoder::ion_writer::{IonWriter, write_mzml_to_ion},
    encoder::utilities::{WriteBytes, SectionStorage},
};

#[cfg(not(all(target_arch = "wasm32", not(target_os = "wasi"))))]
pub use ion::{FileSource, FileWriter};
#[cfg(not(all(target_arch = "wasm32", not(target_os = "wasi"))))]
pub use ion::encoder::utilities::TempFile;
pub use mzml::{BinToMzmlError, bin_to_mzml, parse_indexed_mzml, parse_mzml};
