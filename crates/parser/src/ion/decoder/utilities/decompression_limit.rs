use crate::ion::{IonError, IonResult};

pub const DEFAULT_MAX_UNCOMPRESSED_SIZE: u64 = 1 << 31;
pub const MAX_COMPRESSION_RATIO: u64 = 32768;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecompressionLimit {
    pub max_uncompressed_size: u64,
}

impl Default for DecompressionLimit {
    fn default() -> Self {
        Self {
            max_uncompressed_size: DEFAULT_MAX_UNCOMPRESSED_SIZE,
        }
    }
}

impl DecompressionLimit {
    pub fn new(max_uncompressed_size: u64) -> Self {
        Self {
            max_uncompressed_size,
        }
    }

    pub fn validate(&self, compressed_len: usize, uncompressed_len: usize) -> IonResult<()> {
        let uncompressed = uncompressed_len as u64;

        if uncompressed > self.max_uncompressed_size {
            return Err(IonError::from(format!(
                "decompression rejected: uncompressed size {uncompressed} exceeds limit {}",
                self.max_uncompressed_size
            )));
        }

        let largest_plausible = (compressed_len as u64).saturating_mul(MAX_COMPRESSION_RATIO);
        if uncompressed > largest_plausible {
            return Err(IonError::from(format!(
                "decompression rejected: declared uncompressed size {uncompressed} is implausible for {compressed_len} compressed bytes (max ratio {MAX_COMPRESSION_RATIO})"
            )));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_high_ratio_within_cap() {
        let limit = DecompressionLimit::default();
        assert!(limit.validate(270, 2 * 1024 * 1024).is_ok());
    }

    #[test]
    fn rejects_over_max_size() {
        let limit = DecompressionLimit::new(1_024);
        assert!(limit.validate(1, 1_025).is_err());
        assert!(limit.validate(1, 1_024).is_ok());
    }

    #[test]
    fn cap_applies_regardless_of_compressed_len() {
        let limit = DecompressionLimit::new(4_096);
        assert!(limit.validate(1_000_000, 4_096).is_ok());
        assert!(limit.validate(1_000_000, 4_097).is_err());
    }

    #[test]
    fn rejects_ratio_above_format_maximum() {
        let limit = DecompressionLimit::default();
        assert!(limit.validate(4, 4 * 32768).is_ok());
        assert!(limit.validate(4, 4 * 32768 + 1).is_err());
    }

    #[test]
    fn rejects_nonzero_size_with_no_compressed_bytes() {
        let limit = DecompressionLimit::default();
        assert!(limit.validate(0, 1).is_err());
        assert!(limit.validate(0, 0).is_ok());
    }
}
