use crate::ion::{IonError, IonResult};

pub const DEFAULT_MAX_UNCOMPRESSED_SIZE: u64 = 1 << 31;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecompressionBudget {
    pub max_uncompressed_size: u64,
}

impl Default for DecompressionBudget {
    fn default() -> Self {
        Self {
            max_uncompressed_size: DEFAULT_MAX_UNCOMPRESSED_SIZE,
        }
    }
}

impl DecompressionBudget {
    pub fn new(max_uncompressed_size: u64) -> Self {
        Self {
            max_uncompressed_size,
        }
    }

    pub fn validate(&self, _compressed_len: usize, uncompressed_len: usize) -> IonResult<()> {
        let uncompressed = uncompressed_len as u64;

        if uncompressed > self.max_uncompressed_size {
            return Err(IonError::from(format!(
                "decompression rejected: uncompressed size {uncompressed} exceeds limit {}",
                self.max_uncompressed_size
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
        let budget = DecompressionBudget::default();
        assert!(budget.validate(270, 2 * 1024 * 1024).is_ok());
    }

    #[test]
    fn rejects_over_max_size() {
        let budget = DecompressionBudget::new(1_024);
        assert!(budget.validate(1, 1_025).is_err());
        assert!(budget.validate(1, 1_024).is_ok());
    }

    #[test]
    fn cap_applies_regardless_of_compressed_len() {
        let budget = DecompressionBudget::new(4_096);
        assert!(budget.validate(0, 4_096).is_ok());
        assert!(budget.validate(0, 4_097).is_err());
    }
}
