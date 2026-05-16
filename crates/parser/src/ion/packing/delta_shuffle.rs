use super::{Dtype, IonResult, Packing, PackingId, PackingInput};
use crate::ion::IonError;

pub(crate) static DELTA_SHUFFLE: DeltaShuffle = DeltaShuffle;
pub(crate) struct DeltaShuffle;

impl Packing for DeltaShuffle {
    fn id(&self) -> PackingId {
        PackingId::DeltaShuffle
    }

    fn supports(&self, dtype: Dtype) -> bool {
        matches!(dtype, Dtype::F64)
    }

    fn encode(&self, input: PackingInput<'_>, out: &mut Vec<u8>) -> IonResult<()> {
        let PackingInput::F64(values) = input else {
            return Err(IonError::from("DeltaShuffle requires F64 input"));
        };
        out.reserve(values.len() * 8);
        let mut prev: u64 = 0;
        for &v in values {
            let bits = v.to_bits();
            out.extend_from_slice(&bits.wrapping_sub(prev).to_le_bytes());
            prev = bits;
        }
        Ok(())
    }

    fn decode(&self, input: &[u8], _dtype: Dtype, out: &mut Vec<u8>) -> IonResult<()> {
        out.reserve(input.len());
        let mut prev: u64 = 0;
        for chunk in input.chunks_exact(8) {
            prev = prev.wrapping_add(u64::from_le_bytes(chunk.try_into().unwrap()));
            out.extend_from_slice(&f64::from_bits(prev).to_le_bytes());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(input: &[f64]) -> Vec<f64> {
        let mut enc = Vec::new();
        DELTA_SHUFFLE
            .encode(PackingInput::F64(input), &mut enc)
            .unwrap();
        let mut dec_bytes = Vec::new();
        DELTA_SHUFFLE
            .decode(&enc, Dtype::F64, &mut dec_bytes)
            .unwrap();
        dec_bytes
            .chunks_exact(8)
            .map(|c| f64::from_le_bytes(c.try_into().unwrap()))
            .collect()
    }

    #[test]
    fn empty_produces_no_output() {
        assert!(roundtrip(&[]).is_empty());
    }

    #[test]
    fn single_element_is_bit_exact() {
        let input = [503.42f64];
        let got = roundtrip(&input);
        assert_eq!(got[0].to_bits(), input[0].to_bits());
    }

    #[test]
    fn two_elements_are_bit_exact() {
        let input = [100.0f64, 200.0];
        let got = roundtrip(&input);
        for (a, b) in got.iter().zip(input.iter()) {
            assert_eq!(a.to_bits(), b.to_bits());
        }
    }

    #[test]
    fn special_values_are_bit_exact() {
        let input = [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -0.0f64, 0.0f64];
        let got = roundtrip(&input);
        for (a, b) in got.iter().zip(input.iter()) {
            assert_eq!(a.to_bits(), b.to_bits());
        }
    }

    #[test]
    fn monotonic_mz_array_is_bit_exact() {
        let input: Vec<f64> = (0..10_000).map(|i| 100.0 + i as f64 * 0.01).collect();
        let mut enc = Vec::new();
        DELTA_SHUFFLE
            .encode(PackingInput::F64(&input), &mut enc)
            .unwrap();
        assert_eq!(enc.len(), input.len() * 8);
        let got = roundtrip(&input);
        for (a, b) in got.iter().zip(input.iter()) {
            assert_eq!(a.to_bits(), b.to_bits());
        }
    }

    #[test]
    fn non_f64_input_returns_error() {
        let mut out = Vec::new();
        let result = DELTA_SHUFFLE.encode(PackingInput::F32(&[1.0f32]), &mut out);
        assert!(result.is_err());
    }

    #[test]
    fn encoded_length_equals_input_byte_length() {
        let input: Vec<f64> = (0..100).map(|i| i as f64).collect();
        let mut enc = Vec::new();
        DELTA_SHUFFLE
            .encode(PackingInput::F64(&input), &mut enc)
            .unwrap();
        assert_eq!(enc.len(), input.len() * 8);
    }

    #[test]
    fn is_not_standalone() {
        assert!(!DELTA_SHUFFLE.is_generic());
    }
}
