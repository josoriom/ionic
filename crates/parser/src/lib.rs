pub(crate) mod accessions;
pub mod deprecated;
pub mod ion;
pub mod mzml;
pub mod utilities;

pub use ion::{
    ByteRange, BytesSource, CallbackSource, ChromatogramSummary, Range, ReadBytes, SpectrumSummary,
    decoder,
    decoder::decode::{Metadatum, MetadatumValue},
    decoder::utilities::spectrum_source::{ScanSource, ScanSummary, TimeUnit},
    encoder,
    encoder::encode::WriteOptions,
    encoder::ion_writer::{IonWriter, write_mzml_to_ion},
    encoder::scan_stream::{MemoryReader, ScanStream},
    encoder::utilities::{SectionStorage, WriteBytes},
};

pub use deprecated::read_old_file_to_mzml;
pub use ion::encoder::encode::DEFAULT_MZ_WINDOW;
#[cfg(not(all(target_arch = "wasm32", not(target_os = "wasi"))))]
pub use ion::encoder::utilities::TempFile;
pub use ion::{
    DataXY, IonError, IonReader, IonResult, ItemSlice, Pixel, ReadOptions, Select, Target, Window,
    open_ranges,
};
#[cfg(not(all(target_arch = "wasm32", not(target_os = "wasi"))))]
pub use ion::{FileSource, FileWriter};
pub use mzml::structs::{Chromatogram, MzML, NumericArray, Spectrum};
pub use mzml::{BinToMzmlError, bin_to_mzml, parse_indexed_mzml, parse_mzml};

#[cfg(not(all(target_arch = "wasm32", not(target_os = "wasi"))))]
pub fn mzml_to_ion(input_path: &std::path::Path, output_path: &std::path::Path) -> IonResult<()> {
    let mzml_bytes =
        std::fs::read(input_path).map_err(|error| IonError::from(error.to_string()))?;
    let mzml = parse_mzml(&mzml_bytes).map_err(|error| IonError::from(error.to_string()))?;
    let mut ion_bytes = Vec::new();
    write_mzml_to_ion(&mzml, WriteOptions::default(), &mut ion_bytes)?;
    std::fs::write(output_path, &ion_bytes).map_err(|error| IonError::from(error.to_string()))?;
    Ok(())
}

#[cfg(not(all(target_arch = "wasm32", not(target_os = "wasi"))))]
pub fn ion_to_mzml(input_path: &std::path::Path, output_path: &std::path::Path) -> IonResult<()> {
    let mut reader = IonReader::open_file(input_path, ReadOptions::default())?;
    let mzml = reader.to_mzml()?;
    let mzml_bytes = bin_to_mzml(&mzml).map_err(|error| IonError::from(error.to_string()))?;
    std::fs::write(output_path, &mzml_bytes).map_err(|error| IonError::from(error.to_string()))?;
    Ok(())
}
