use super::{DeltaWord, Dtype, IonResult, Packing, PackingId, PackingInput};
use crate::ion::IonError;

pub(crate) static DELTA_SHUFFLE: DeltaShuffle = DeltaShuffle;
pub(crate) struct DeltaShuffle;

fn encode_delta<W: DeltaWord>(bits: impl Iterator<Item = W>, out: &mut Vec<u8>) {
    let mut prev = W::default();
    for w in bits {
        DeltaWord::wrapping_sub(w, prev).to_le_bytes_into(out);
        prev = w;
    }
}

fn decode_delta<W: DeltaWord>(input: &[u8], mut emit: impl FnMut(W)) {
    let mut prev = W::default();
    for chunk in input.chunks_exact(W::BYTES) {
        prev = DeltaWord::wrapping_add(prev, W::from_le_chunk(chunk));
        emit(prev);
    }
}

impl Packing for DeltaShuffle {
    fn id(&self) -> PackingId {
        PackingId::DeltaShuffle
    }

    fn supports(&self, dtype: Dtype) -> bool {
        matches!(dtype, Dtype::F64)
    }

    fn encode(&self, input: PackingInput<'_>, out: &mut Vec<u8>) -> IonResult<()> {
        match input {
            PackingInput::F64(v) => {
                out.reserve(v.len() * 8);
                encode_delta::<u64>(v.iter().map(|x| x.to_bits()), out);
                Ok(())
            }
            _ => Err(IonError::from("delta filter needs f64 input")),
        }
    }

    fn decode(&self, input: &[u8], dtype: Dtype, out: &mut Vec<u8>) -> IonResult<()> {
        match dtype {
            Dtype::F64 => {
                out.reserve(input.len());
                decode_delta::<u64>(input, |w| {
                    out.extend_from_slice(&f64::from_bits(w).to_le_bytes())
                });
                Ok(())
            }
            _ => Err(IonError::BadDtype {
                dtype: dtype as u8,
                kind: "delta filter needs f64",
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip_f64(input: &[f64]) -> Vec<f64> {
        let mut enc = Vec::new();
        DELTA_SHUFFLE
            .encode(PackingInput::F64(input), &mut enc)
            .unwrap();
        let mut dec = Vec::new();
        DELTA_SHUFFLE.decode(&enc, Dtype::F64, &mut dec).unwrap();
        dec.chunks_exact(8)
            .map(|c| f64::from_le_bytes(c.try_into().unwrap()))
            .collect()
    }

    #[test]
    fn empty_produces_no_output() {
        assert!(roundtrip_f64(&[]).is_empty());
    }

    #[test]
    fn single_element_f64_is_bit_exact() {
        let input = [503.42f64];
        let got = roundtrip_f64(&input);
        assert_eq!(got[0].to_bits(), input[0].to_bits());
    }

    #[test]
    fn two_elements_f64_are_bit_exact() {
        let input = [100.0f64, 200.0];
        let got = roundtrip_f64(&input);
        for (a, b) in got.iter().zip(input.iter()) {
            assert_eq!(a.to_bits(), b.to_bits());
        }
    }

    #[test]
    fn special_values_f64_are_bit_exact() {
        let input = [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -0.0f64, 0.0f64];
        let got = roundtrip_f64(&input);
        for (a, b) in got.iter().zip(input.iter()) {
            assert_eq!(a.to_bits(), b.to_bits());
        }
    }

    #[test]
    fn monotonic_mz_f64_is_bit_exact() {
        let input: Vec<f64> = (0..10_000).map(|i| 100.0 + i as f64 * 0.01).collect();
        let got = roundtrip_f64(&input);
        for (a, b) in got.iter().zip(input.iter()) {
            assert_eq!(a.to_bits(), b.to_bits());
        }
    }

    #[test]
    fn encoded_length_f64_equals_input_bytes() {
        let input: Vec<f64> = (0..100).map(|i| i as f64).collect();
        let mut enc = Vec::new();
        DELTA_SHUFFLE
            .encode(PackingInput::F64(&input), &mut enc)
            .unwrap();
        assert_eq!(enc.len(), input.len() * 8);
    }

    #[test]
    fn packing_for_dtype_dispatch() {
        use super::super::packing_for;
        use crate::accessions::MZ_ARRAY;
        assert_eq!(
            packing_for(MZ_ARRAY, Dtype::F64, 100).id(),
            PackingId::DeltaShuffle
        );
        assert_eq!(packing_for(0, Dtype::I32, 1).id(), PackingId::Raw);
    }
}
