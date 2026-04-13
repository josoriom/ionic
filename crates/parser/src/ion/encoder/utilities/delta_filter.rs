pub(crate) fn encode_f64(input: &[f64], out: &mut Vec<u8>) {
    out.reserve(input.len() * 8);
    let mut prev: u64 = 0;
    for &v in input {
        let bits = v.to_bits();
        out.extend_from_slice(&bits.wrapping_sub(prev).to_le_bytes());
        prev = bits;
    }
}

pub(crate) fn decode_f64(input: &[u8], out: &mut Vec<f64>) {
    out.reserve(input.len() / 8);
    let mut prev: u64 = 0;
    for chunk in input.chunks_exact(8) {
        prev = prev.wrapping_add(u64::from_le_bytes(chunk.try_into().unwrap()));
        out.push(f64::from_bits(prev));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(input: &[f64]) -> Vec<f64> {
        let mut enc = Vec::new();
        encode_f64(input, &mut enc);
        let mut dec = Vec::new();
        decode_f64(&enc, &mut dec);
        dec
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
        encode_f64(&input, &mut enc);
        assert_eq!(enc.len(), input.len() * 8);
        let got = roundtrip(&input);
        for (a, b) in got.iter().zip(input.iter()) {
            assert_eq!(a.to_bits(), b.to_bits());
        }
    }
}
