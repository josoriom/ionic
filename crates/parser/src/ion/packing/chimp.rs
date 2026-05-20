use super::{DeltaWord, Dtype, IonResult, Packing, PackingId, PackingInput};
use crate::ion::IonError;

pub(crate) static CHIMP: Chimp = Chimp;
pub(crate) struct Chimp;

impl Packing for Chimp {
    fn id(&self) -> PackingId {
        PackingId::Chimp
    }

    fn is_variable_length(&self) -> bool {
        true
    }

    fn min_input_len(&self) -> usize {
        2
    }

    fn supports(&self, dtype: Dtype) -> bool {
        matches!(dtype, Dtype::F32 | Dtype::F64)
    }

    fn encode(&self, input: PackingInput<'_>, out: &mut Vec<u8>) -> IonResult<()> {
        match input {
            PackingInput::F64(v) => encode::<u64>(v.iter().map(|x| x.to_bits()), v.len(), out),
            PackingInput::F32(v) => encode::<u32>(v.iter().map(|x| x.to_bits()), v.len(), out),
            _ => Err(IonError::from("Chimp requires F32 or F64 input")),
        }
    }

    fn decode(&self, input: &[u8], dtype: Dtype, out: &mut Vec<u8>) -> IonResult<()> {
        match dtype {
            Dtype::F64 => decode::<u64>(input, out, |w, o| {
                o.extend_from_slice(&f64::from_bits(w).to_le_bytes())
            }),
            Dtype::F32 => decode::<u32>(input, out, |w, o| {
                o.extend_from_slice(&f32::from_bits(w).to_le_bytes())
            }),
            _ => Err(IonError::from("Chimp: unsupported dtype")),
        }
    }
}

fn encode<W: DeltaWord>(
    mut bits: impl Iterator<Item = W>,
    len: usize,
    out: &mut Vec<u8>,
) -> IonResult<()> {
    if len == 0 {
        return Ok(());
    }
    let n = u32::try_from(len)
        .map_err(|_| IonError::from("Chimp: input exceeds u32 element count"))?;
    out.extend_from_slice(&n.to_le_bytes());

    let first = bits.next().unwrap();
    first.to_le_bytes_into(out);
    let mut prev = first;

    for cur in bits {
        let xor = DeltaWord::bitxor(cur, prev);
        if xor.to_u64() == 0 {
            out.push(0xFF);
        } else {
            let lz = ((xor.leading_zeros() / 8) as u8).min(W::BYTES as u8 - 1);
            let tz = ((xor.trailing_zeros() / 8) as u8).min(W::BYTES as u8 - 1);
            let sig_bytes = (W::BYTES - lz as usize - tz as usize).max(1) as u8;
            let tz_actual = W::BYTES - lz as usize - sig_bytes as usize;
            let header = (lz << 3) | (sig_bytes - 1);
            out.push(header);
            let shifted = xor.shr_bytes(tz_actual);
            for i in 0..sig_bytes as usize {
                out.push(shifted.byte_at(i));
            }
        }
        prev = cur;
    }
    Ok(())
}

fn decode<W: DeltaWord>(
    input: &[u8],
    out: &mut Vec<u8>,
    write_word: impl Fn(W, &mut Vec<u8>),
) -> IonResult<()> {
    let header_size = 4 + W::BYTES;
    if input.len() < header_size {
        return Err(IonError::from("Chimp: truncated header"));
    }
    let n = u32::from_le_bytes(input[0..4].try_into().unwrap()) as usize;
    let first = W::from_le_chunk(&input[4..4 + W::BYTES]);

    write_word(first, out);
    let mut prev = first;
    let mut pos = header_size;

    for _ in 1..n {
        if pos >= input.len() {
            return Err(IonError::from("Chimp: truncated data"));
        }
        let header = input[pos];
        pos += 1;
        if header == 0xFF {
            write_word(prev, out);
        } else {
            let max_lz = W::BYTES - 1;
            if (header >> 3) as usize > max_lz {
                return Err(IonError::from("Chimp: invalid header bits"));
            }
            let lz = (header >> 3) as usize;
            let sig_bytes = ((header & 0x07) + 1) as usize;
            if lz + sig_bytes > W::BYTES {
                return Err(IonError::from("Chimp: invalid lz/sig_bytes combination"));
            }
            let tz = W::BYTES - lz - sig_bytes;
            if pos + sig_bytes > input.len() {
                return Err(IonError::from("Chimp: truncated significant bytes"));
            }
            let mut xor = W::default();
            for i in 0..sig_bytes {
                xor = DeltaWord::bitxor(xor, W::from_u64((input[pos + i] as u64) << (i * 8)));
            }
            xor = xor.shl_bytes(tz);
            pos += sig_bytes;
            let cur = DeltaWord::bitxor(xor, prev);
            write_word(cur, out);
            prev = cur;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip_f64(vals: &[f64]) -> Vec<f64> {
        let mut enc = Vec::new();
        CHIMP.encode(PackingInput::F64(vals), &mut enc).unwrap();
        let mut dec = Vec::new();
        CHIMP.decode(&enc, Dtype::F64, &mut dec).unwrap();
        dec.chunks_exact(8).map(|c| f64::from_le_bytes(c.try_into().unwrap())).collect()
    }

    fn roundtrip_f32(vals: &[f32]) -> Vec<f32> {
        let mut enc = Vec::new();
        CHIMP.encode(PackingInput::F32(vals), &mut enc).unwrap();
        let mut dec = Vec::new();
        CHIMP.decode(&enc, Dtype::F32, &mut dec).unwrap();
        dec.chunks_exact(4).map(|c| f32::from_le_bytes(c.try_into().unwrap())).collect()
    }

    #[test]
    fn identical_values_f64() {
        let vals = vec![1.5f64; 50];
        let got = roundtrip_f64(&vals);
        for (a, b) in got.iter().zip(vals.iter()) { assert_eq!(a.to_bits(), b.to_bits()); }
    }

    #[test]
    fn identical_values_f32() {
        let vals = vec![1.5f32; 50];
        let got = roundtrip_f32(&vals);
        for (a, b) in got.iter().zip(vals.iter()) { assert_eq!(a.to_bits(), b.to_bits()); }
    }

    #[test]
    fn monotonic_values_f64() {
        let vals: Vec<f64> = (0..100).map(|i| i as f64 * 0.1).collect();
        let got = roundtrip_f64(&vals);
        for (a, b) in got.iter().zip(vals.iter()) { assert_eq!(a.to_bits(), b.to_bits()); }
    }

    #[test]
    fn monotonic_values_f32() {
        let vals: Vec<f32> = (0..100).map(|i| i as f32 * 0.1).collect();
        let got = roundtrip_f32(&vals);
        for (a, b) in got.iter().zip(vals.iter()) { assert_eq!(a.to_bits(), b.to_bits()); }
    }

    #[test]
    fn special_values_f64() {
        let vals = vec![0.0f64, f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -0.0f64];
        let got = roundtrip_f64(&vals);
        for (a, b) in got.iter().zip(vals.iter()) { assert_eq!(a.to_bits(), b.to_bits()); }
    }

    #[test]
    fn special_values_f32() {
        let vals = vec![0.0f32, f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -0.0f32];
        let got = roundtrip_f32(&vals);
        for (a, b) in got.iter().zip(vals.iter()) { assert_eq!(a.to_bits(), b.to_bits()); }
    }

    #[test]
    fn rejects_malformed_header_f64() {
        let mut input = Vec::new();
        input.extend_from_slice(&2u32.to_le_bytes());
        input.extend_from_slice(&1.0f64.to_bits().to_le_bytes());
        input.push(0x40);
        input.push(0x00);
        let mut out = Vec::new();
        assert!(CHIMP.decode(&input, Dtype::F64, &mut out).is_err());
    }

    #[test]
    fn rejects_invalid_lz_sig_combination_f64() {
        let mut input = Vec::new();
        input.extend_from_slice(&2u32.to_le_bytes());
        input.extend_from_slice(&1.0f64.to_bits().to_le_bytes());
        input.push((7u8 << 3) | 7);
        input.extend_from_slice(&[0u8; 8]);
        let mut out = Vec::new();
        assert!(CHIMP.decode(&input, Dtype::F64, &mut out).is_err());
    }
}
