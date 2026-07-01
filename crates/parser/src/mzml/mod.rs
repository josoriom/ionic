pub mod parse_mzml;
pub use parse_mzml::{parse_indexed_mzml, parse_mzml};
pub mod stream_reader;
pub use stream_reader::MzmlReader;
pub mod bin_to_mzml;
pub use bin_to_mzml::{BinToMzmlError, bin_to_mzml};
pub mod schema;
pub mod structs;
pub mod utilities;

#[cfg(test)]
mod tests;
