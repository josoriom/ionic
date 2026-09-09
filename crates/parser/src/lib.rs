pub(crate) mod accessions;
pub mod convert;
pub mod ion;
pub mod mzml;
pub mod utilities;

pub use convert::{ConvertKind, ConvertOptions, convert};
pub use ion::{
    ArrayAddress, ArrayKind, ByteRange, BytesSource, CallbackSource, ChromatogramSummary, DataXY,
    IonError, IonReader, IonResult, ItemKind, ItemSlice, Pixel, Query, Range, ReadBytes,
    ReadOptions, Select, SourceBytes, SpectrumSummary, Window, coalesce_byte_ranges,
    decoder::decode::{Metadatum, MetadatumValue},
    encoder::{
        encode::{DEFAULT_MZ_WINDOW, WriteOptions},
        ion_writer::{IonWriter, write_mzml_to_ion},
        scan_stream::{MemoryReader, ScanStream},
        utilities::{SectionStorage, WriteBytes},
    },
    open_ranges,
    scan::{ScanSource, ScanSummary, TimeUnit},
};
#[cfg(not(all(target_arch = "wasm32", not(target_os = "wasi"))))]
pub use ion::{FileSource, FileWriter};
pub use mzml::{
    BinToMzmlError, ParseError, bin_to_mzml, parse_indexed_mzml, parse_mzml,
    structs::{Chromatogram, MzML, NumericArray, Spectrum},
};
