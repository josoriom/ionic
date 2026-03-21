pub mod mzml;
pub use mzml::{bin_to_mzml, parse_indexed_mzml, parse_mzml, structs::*};
pub mod ion;
pub use ion::{decoder, encoder, utilities::Header};
pub mod utilities;
