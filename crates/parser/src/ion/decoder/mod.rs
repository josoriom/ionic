pub mod decode;
pub use decode::{IonReader, ReadOptions};
pub(crate) mod utilities;

#[cfg(test)]
mod tests;
