use super::{Dtype, IonResult, Packing, PackingId, PackingInput};
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
        matches!(dtype, Dtype::F64)
    }

    fn encode(&self, input: PackingInput<'_>, out: &mut Vec<u8>) -> IonResult<()> {
        match input {
            PackingInput::F64(values) => encode_f64(values, out),
            _ => Err(IonError::from("Chimp requires F64 input")),
        }
    }

    fn decode(&self, input: &[u8], dtype: Dtype, out: &mut Vec<u8>) -> IonResult<()> {
        match dtype {
            Dtype::F64 => decode_f64(input, out),
            _ => Err(IonError::from("Chimp: unsupported dtype")),
        }
    }
}

fn encode_f64(values: &[f64], out: &mut Vec<u8>) -> IonResult<()> {
    if values.is_empty() {
        return Ok(());
    }
    let n = u32::try_from(values.len())
        .map_err(|_| IonError::from("Chimp: input exceeds u32 element count"))?;
    out.extend_from_slice(&n.to_le_bytes());

    let mut prev_bits = values[0].to_bits();
    out.extend_from_slice(&prev_bits.to_le_bytes());

    for &v in values.iter().skip(1) {
        let cur_bits = v.to_bits();
        let xor = cur_bits ^ prev_bits;
        if xor == 0 {
            out.push(0xFF);
        } else {
            let lz = (xor.leading_zeros() / 8) as u8;
            let tz = (xor.trailing_zeros() / 8) as u8;
            let lz = lz.min(7);
            let tz = tz.min(7);
            let sig_bytes = 8 - lz as usize - tz as usize;
            let sig_bytes = sig_bytes.max(1) as u8;
            let tz_actual = 8 - lz as usize - sig_bytes as usize;
            let header = (lz << 3) | (sig_bytes - 1);
            out.push(header);
            let shifted = xor >> (tz_actual * 8);
            for i in 0..sig_bytes as usize {
                out.push((shifted >> (i * 8)) as u8);
            }
        }
        prev_bits = cur_bits;
    }
    Ok(())
}

fn decode_f64(input: &[u8], out: &mut Vec<u8>) -> IonResult<()> {
    if input.len() < 12 {
        return Err(IonError::from("Chimp: truncated header"));
    }
    let n = u32::from_le_bytes(input[0..4].try_into().unwrap()) as usize;
    let first_bits = u64::from_le_bytes(input[4..12].try_into().unwrap());

    out.extend_from_slice(&f64::from_bits(first_bits).to_le_bytes());

    let mut prev_bits = first_bits;
    let mut pos = 12usize;

    for _ in 1..n {
        if pos >= input.len() {
            return Err(IonError::from("Chimp: truncated data"));
        }
        let header = input[pos];
        pos += 1;
        if header == 0xFF {
            out.extend_from_slice(&f64::from_bits(prev_bits).to_le_bytes());
        } else {
            if header & 0xC0 != 0 {
                return Err(IonError::from("Chimp: invalid header bits"));
            }
            let lz = (header >> 3) as usize;
            let sig_bytes = ((header & 0x07) + 1) as usize;
            if lz + sig_bytes > 8 {
                return Err(IonError::from("Chimp: invalid lz/sig_bytes combination"));
            }
            let tz = 8 - lz - sig_bytes;
            if pos + sig_bytes > input.len() {
                return Err(IonError::from("Chimp: truncated significant bytes"));
            }
            let mut xor = 0u64;
            for i in 0..sig_bytes {
                xor |= (input[pos + i] as u64) << (i * 8);
            }
            xor <<= tz * 8;
            pos += sig_bytes;
            let cur_bits = xor ^ prev_bits;
            out.extend_from_slice(&f64::from_bits(cur_bits).to_le_bytes());
            prev_bits = cur_bits;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(vals: &[f64]) -> Vec<f64> {
        let mut enc = Vec::new();
        encode_f64(vals, &mut enc).unwrap();
        let mut dec = Vec::new();
        decode_f64(&enc, &mut dec).unwrap();
        dec.chunks_exact(8)
            .map(|c| f64::from_le_bytes(c.try_into().unwrap()))
            .collect()
    }

    #[test]
    fn identical_values() {
        let vals = vec![1.5f64; 50];
        let got = roundtrip(&vals);
        for (a, b) in got.iter().zip(vals.iter()) {
            assert_eq!(a.to_bits(), b.to_bits());
        }
    }

    #[test]
    fn monotonic_values() {
        let vals: Vec<f64> = (0..100).map(|i| i as f64 * 0.1).collect();
        let got = roundtrip(&vals);
        for (a, b) in got.iter().zip(vals.iter()) {
            assert_eq!(a.to_bits(), b.to_bits());
        }
    }

    #[test]
    fn rejects_malformed_header() {
        let mut input = Vec::new();
        input.extend_from_slice(&2u32.to_le_bytes());
        input.extend_from_slice(&1.0f64.to_bits().to_le_bytes());
        input.push(0x40);
        input.push(0x00);
        let mut out = Vec::new();
        assert!(decode_f64(&input, &mut out).is_err());
    }

    #[test]
    fn rejects_invalid_lz_sig_combination() {
        let mut input = Vec::new();
        input.extend_from_slice(&2u32.to_le_bytes());
        input.extend_from_slice(&1.0f64.to_bits().to_le_bytes());
        input.push((7u8 << 3) | 7);
        input.extend_from_slice(&[0u8; 8]);
        let mut out = Vec::new();
        assert!(decode_f64(&input, &mut out).is_err());
    }

    #[test]
    fn special_values() {
        let vals = vec![0.0f64, f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -0.0f64];
        let got = roundtrip(&vals);
        for (a, b) in got.iter().zip(vals.iter()) {
            assert_eq!(a.to_bits(), b.to_bits());
        }
    }
}
