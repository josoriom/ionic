pub(crate) mod accessions;
pub mod ion;
pub mod legacy;
pub mod mzml;
pub mod utilities;

pub use ion::{
    ByteRange, BytesSource, CallbackSource, ChromatogramSummary, DataXY, IonError, IonReader,
    IonResult, ItemKind, ItemSlice, Pixel, Range, ReadBytes, ReadOptions, Select, SpectrumSummary,
    Window,
    decoder::{
        decode::{Metadatum, MetadatumValue},
        utilities::spectrum_source::{ScanSource, ScanSummary, TimeUnit},
    },
    encoder::{
        encode::{DEFAULT_MZ_WINDOW, WriteOptions},
        ion_writer::{IonWriter, write_mzml_to_ion},
        scan_stream::{MemoryReader, ScanStream},
        utilities::{SectionStorage, WriteBytes},
    },
    open_ranges,
};
#[cfg(not(all(target_arch = "wasm32", not(target_os = "wasi"))))]
pub use ion::{FileSource, FileWriter};
pub use legacy::{read_old_file_to_mzml, upgrade_old_ion};
pub use mzml::{
    BinToMzmlError, ParseError, bin_to_mzml, parse_indexed_mzml, parse_mzml,
    structs::{Chromatogram, MzML, NumericArray, Spectrum},
};

#[cfg(not(all(target_arch = "wasm32", not(target_os = "wasi"))))]
pub fn mzml_to_ion(input_path: &std::path::Path, output_path: &std::path::Path) -> IonResult<()> {
    let mzml_bytes = std::fs::read(input_path)?;
    let mzml = parse_mzml(&mzml_bytes)?;
    let mut ion_bytes = Vec::new();
    write_mzml_to_ion(&mzml, WriteOptions::default(), &mut ion_bytes)?;
    std::fs::write(output_path, &ion_bytes)?;
    Ok(())
}

#[cfg(not(all(target_arch = "wasm32", not(target_os = "wasi"))))]
pub fn ion_to_mzml(input_path: &std::path::Path, output_path: &std::path::Path) -> IonResult<()> {
    let mut reader = IonReader::open_file(input_path, ReadOptions::default())?;
    let mzml = reader.to_mzml()?;
    let mzml_bytes = bin_to_mzml(&mzml)?;
    std::fs::write(output_path, &mzml_bytes)?;
    Ok(())
}
