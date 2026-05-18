use super::{Dtype, IonResult, Packing, PackingId, PackingInput};
use crate::ion::IonError;

pub(crate) static ALP: Alp = Alp;
pub(crate) struct Alp;

impl Packing for Alp {
    fn id(&self) -> PackingId {
        PackingId::Alp
    }

    fn is_variable_length(&self) -> bool {
        true
    }

    fn min_input_len(&self) -> usize {
        64
    }

    fn supports(&self, dtype: Dtype) -> bool {
        matches!(dtype, Dtype::F32 | Dtype::F64)
    }

    fn encode(&self, input: PackingInput<'_>, out: &mut Vec<u8>) -> IonResult<()> {
        match input {
            PackingInput::F64(values) => encode_f64(values, out),
            PackingInput::F32(values) => encode_f32(values, out),
            _ => Err(IonError::from("ALP requires F32 or F64 input")),
        }
    }

    fn decode(&self, input: &[u8], dtype: Dtype, out: &mut Vec<u8>) -> IonResult<()> {
        match dtype {
            Dtype::F64 => decode_f64(input, out),
            Dtype::F32 => decode_f32(input, out),
            _ => Err(IonError::from("ALP: unsupported dtype")),
        }
    }
}


const P10_F64: [f64; 19] = [
    1e0, 1e1, 1e2, 1e3, 1e4, 1e5, 1e6, 1e7, 1e8, 1e9, 1e10, 1e11, 1e12, 1e13, 1e14, 1e15,
    1e16, 1e17, 1e18,
];
const MAX_FACTOR_F64: u8 = 18;
const SAMPLE_SIZE: usize = 512;

fn can_encode_exact_f64(v: f64, factor: u8) -> bool {
    if !v.is_finite() {
        return false;
    }
    let scale = P10_F64[factor as usize];
    let scaled = v * scale;
    if scaled.abs() > i64::MAX as f64 {
        return false;
    }
    let int_val = scaled.round() as i64;
    (int_val as f64) / scale == v
}

fn find_factor_f64(values: &[f64]) -> u8 {
    let step = (values.len() / SAMPLE_SIZE).max(1);
    let mut best_factor = 0u8;
    let mut best_count = 0usize;
    for f in 0..=MAX_FACTOR_F64 {
        let count = values.iter().step_by(step).filter(|&&v| can_encode_exact_f64(v, f)).count();
        if count > best_count {
            best_count = count;
            best_factor = f;
            if count == values.iter().step_by(step).count() {
                break;
            }
        }
    }
    best_factor
}

fn encode_f64(values: &[f64], out: &mut Vec<u8>) -> IonResult<()> {
    let factor = find_factor_f64(values);
    let scale = P10_F64[factor as usize];
    let n_u32 = u32::try_from(values.len())
        .map_err(|_| IonError::from("ALP F64: input exceeds u32 element count"))?;

    let mut ints = Vec::with_capacity(values.len());
    let mut exc_positions: Vec<u32> = Vec::new();
    let mut exc_bits: Vec<u64> = Vec::new();

    for (i, &v) in values.iter().enumerate() {
        if can_encode_exact_f64(v, factor) {
            ints.push(((v * scale).round()) as i64);
        } else {
            ints.push(0);
            exc_positions.push(i as u32);
            exc_bits.push(v.to_bits());
        }
    }

    let exc_u32 = u32::try_from(exc_positions.len())
        .map_err(|_| IonError::from("ALP F64: exception count exceeds u32"))?;

    out.push(factor);
    out.extend_from_slice(&n_u32.to_le_bytes());
    out.extend_from_slice(&exc_u32.to_le_bytes());
    write_delta_vbyte(&ints, out)?;

    for pos in &exc_positions {
        out.extend_from_slice(&pos.to_le_bytes());
    }
    for bits in &exc_bits {
        out.extend_from_slice(&bits.to_le_bytes());
    }

    Ok(())
}

fn decode_f64(input: &[u8], out: &mut Vec<u8>) -> IonResult<()> {
    if input.len() < 9 {
        return Err(IonError::from("ALP F64: truncated header"));
    }
    let factor = input[0];
    if factor > MAX_FACTOR_F64 {
        return Err(IonError::from("ALP F64: invalid factor"));
    }
    let n = u32::from_le_bytes(input[1..5].try_into().unwrap()) as usize;
    let exc_count = u32::from_le_bytes(input[5..9].try_into().unwrap()) as usize;
    if exc_count > n {
        return Err(IonError::from("ALP F64: exception count exceeds element count"));
    }
    let scale = P10_F64[factor as usize];

    let (ints, consumed) = read_delta_vbyte(&input[9..], n)?;
    let cursor = 9 + consumed;

    let exc_pos_end = cursor
        .checked_add(exc_count.checked_mul(4).ok_or_else(|| IonError::from("ALP F64: exception index overflow"))?)
        .ok_or_else(|| IonError::from("ALP F64: exception index overflow"))?;
    let exc_val_end = exc_pos_end
        .checked_add(exc_count.checked_mul(8).ok_or_else(|| IonError::from("ALP F64: exception value overflow"))?)
        .ok_or_else(|| IonError::from("ALP F64: exception value overflow"))?;
    if input.len() < exc_val_end {
        return Err(IonError::from("ALP F64: truncated exceptions"));
    }

    let mut values: Vec<f64> = ints.iter().map(|&i| i as f64 / scale).collect();

    for j in 0..exc_count {
        let pos = u32::from_le_bytes(
            input[cursor + j * 4..cursor + j * 4 + 4].try_into().unwrap(),
        ) as usize;
        let bits = u64::from_le_bytes(
            input[exc_pos_end + j * 8..exc_pos_end + j * 8 + 8].try_into().unwrap(),
        );
        if pos >= values.len() {
            return Err(IonError::from("ALP F64: exception position out of bounds"));
        }
        values[pos] = f64::from_bits(bits);
    }

    out.reserve(n * 8);
    for v in values {
        out.extend_from_slice(&v.to_le_bytes());
    }
    Ok(())
}


const P10_F32: [f32; 11] = [1e0, 1e1, 1e2, 1e3, 1e4, 1e5, 1e6, 1e7, 1e8, 1e9, 1e10];
const MAX_FACTOR_F32: u8 = 10;

fn can_encode_exact_f32(v: f32, factor: u8) -> bool {
    if !v.is_finite() {
        return false;
    }
    let scale = P10_F32[factor as usize];
    let scaled = v * scale;
    if scaled.abs() > i32::MAX as f32 {
        return false;
    }
    let int_val = scaled.round() as i32;
    (int_val as f32) / scale == v
}

fn find_factor_f32(values: &[f32]) -> u8 {
    let step = (values.len() / SAMPLE_SIZE).max(1);
    let mut best_factor = 0u8;
    let mut best_count = 0usize;
    for f in 0..=MAX_FACTOR_F32 {
        let count = values.iter().step_by(step).filter(|&&v| can_encode_exact_f32(v, f)).count();
        if count > best_count {
            best_count = count;
            best_factor = f;
            if count == values.iter().step_by(step).count() {
                break;
            }
        }
    }
    best_factor
}

fn encode_f32(values: &[f32], out: &mut Vec<u8>) -> IonResult<()> {
    let factor = find_factor_f32(values);
    let scale = P10_F32[factor as usize];
    let n_u32 = u32::try_from(values.len())
        .map_err(|_| IonError::from("ALP F32: input exceeds u32 element count"))?;

    let mut ints = Vec::with_capacity(values.len());
    let mut exc_positions: Vec<u32> = Vec::new();
    let mut exc_bits: Vec<u32> = Vec::new();

    for (i, &v) in values.iter().enumerate() {
        if can_encode_exact_f32(v, factor) {
            ints.push(((v * scale).round()) as i32 as i64);
        } else {
            ints.push(0);
            exc_positions.push(i as u32);
            exc_bits.push(v.to_bits());
        }
    }

    let exc_u32 = u32::try_from(exc_positions.len())
        .map_err(|_| IonError::from("ALP F32: exception count exceeds u32"))?;

    out.push(factor);
    out.extend_from_slice(&n_u32.to_le_bytes());
    out.extend_from_slice(&exc_u32.to_le_bytes());
    write_delta_vbyte(&ints, out)?;

    for pos in &exc_positions {
        out.extend_from_slice(&pos.to_le_bytes());
    }
    for bits in &exc_bits {
        out.extend_from_slice(&bits.to_le_bytes());
    }

    Ok(())
}

fn decode_f32(input: &[u8], out: &mut Vec<u8>) -> IonResult<()> {
    if input.len() < 9 {
        return Err(IonError::from("ALP F32: truncated header"));
    }
    let factor = input[0];
    if factor > MAX_FACTOR_F32 {
        return Err(IonError::from("ALP F32: invalid factor"));
    }
    let n = u32::from_le_bytes(input[1..5].try_into().unwrap()) as usize;
    let exc_count = u32::from_le_bytes(input[5..9].try_into().unwrap()) as usize;
    if exc_count > n {
        return Err(IonError::from("ALP F32: exception count exceeds element count"));
    }
    let scale = P10_F32[factor as usize];

    let (ints, consumed) = read_delta_vbyte(&input[9..], n)?;
    let cursor = 9 + consumed;

    let exc_pos_end = cursor
        .checked_add(exc_count.checked_mul(4).ok_or_else(|| IonError::from("ALP F32: exception index overflow"))?)
        .ok_or_else(|| IonError::from("ALP F32: exception index overflow"))?;
    let exc_val_end = exc_pos_end
        .checked_add(exc_count.checked_mul(4).ok_or_else(|| IonError::from("ALP F32: exception value overflow"))?)
        .ok_or_else(|| IonError::from("ALP F32: exception value overflow"))?;
    if input.len() < exc_val_end {
        return Err(IonError::from("ALP F32: truncated exceptions"));
    }

    let mut values: Vec<f32> = ints.iter().map(|&i| i as f32 / scale).collect();

    for j in 0..exc_count {
        let pos = u32::from_le_bytes(
            input[cursor + j * 4..cursor + j * 4 + 4].try_into().unwrap(),
        ) as usize;
        let bits = u32::from_le_bytes(
            input[exc_pos_end + j * 4..exc_pos_end + j * 4 + 4].try_into().unwrap(),
        );
        if pos >= values.len() {
            return Err(IonError::from("ALP F32: exception position out of bounds"));
        }
        values[pos] = f32::from_bits(bits);
    }

    out.reserve(n * 4);
    for v in values {
        out.extend_from_slice(&v.to_le_bytes());
    }
    Ok(())
}


fn write_delta_vbyte(values: &[i64], out: &mut Vec<u8>) -> IonResult<()> {
    let len_pos = out.len();
    out.extend_from_slice(&[0u8; 4]);
    let start = out.len();

    let mut prev = 0i64;
    for &v in values {
        let delta = v.wrapping_sub(prev);
        prev = v;
        let z = ((delta << 1) ^ (delta >> 63)) as u64;
        write_vbyte(z, out);
    }

    let written = u32::try_from(out.len() - start)
        .map_err(|_| IonError::from("ALP: VByte payload exceeds 4 GiB"))?;
    out[len_pos..len_pos + 4].copy_from_slice(&written.to_le_bytes());
    Ok(())
}

fn read_delta_vbyte(input: &[u8], count: usize) -> IonResult<(Vec<i64>, usize)> {
    if input.len() < 4 {
        return Err(IonError::from("ALP: truncated VByte length"));
    }
    let vbyte_len = u32::from_le_bytes(input[..4].try_into().unwrap()) as usize;
    if input.len() < 4 + vbyte_len {
        return Err(IonError::from("ALP: truncated VByte data"));
    }
    if count > vbyte_len {
        return Err(IonError::from("ALP: count exceeds plausible bound for VByte payload"));
    }

    let mut values = Vec::with_capacity(count);
    let mut cursor = 4usize;
    let mut prev = 0i64;

    for _ in 0..count {
        let (z, bytes) = read_vbyte(&input[cursor..])?;
        let delta = ((z >> 1) as i64) ^ -((z & 1) as i64);
        prev = prev.wrapping_add(delta);
        values.push(prev);
        cursor += bytes;
    }

    Ok((values, 4 + vbyte_len))
}

fn write_vbyte(mut v: u64, out: &mut Vec<u8>) {
    loop {
        let byte = (v & 0x7F) as u8;
        v >>= 7;
        if v == 0 {
            out.push(byte);
            break;
        }
        out.push(byte | 0x80);
    }
}

fn read_vbyte(input: &[u8]) -> IonResult<(u64, usize)> {
    let mut result = 0u64;
    let mut shift = 0u32;
    for (i, &byte) in input.iter().enumerate() {
        result |= ((byte & 0x7F) as u64) << shift;
        if byte & 0x80 == 0 {
            return Ok((result, i + 1));
        }
        shift += 7;
        if shift >= 64 {
            return Err(IonError::from("ALP: VByte overflow"));
        }
    }
    Err(IonError::from("ALP: truncated VByte sequence"))
}


#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip_f64(input: &[f64]) -> Vec<f64> {
        let mut enc = Vec::new();
        ALP.encode(PackingInput::F64(input), &mut enc).unwrap();
        let mut dec = Vec::new();
        ALP.decode(&enc, Dtype::F64, &mut dec).unwrap();
        dec.chunks_exact(8).map(|c| f64::from_le_bytes(c.try_into().unwrap())).collect()
    }

    fn roundtrip_f32(input: &[f32]) -> Vec<f32> {
        let mut enc = Vec::new();
        ALP.encode(PackingInput::F32(input), &mut enc).unwrap();
        let mut dec = Vec::new();
        ALP.decode(&enc, Dtype::F32, &mut dec).unwrap();
        dec.chunks_exact(4).map(|c| f32::from_le_bytes(c.try_into().unwrap())).collect()
    }

    #[test]
    fn f64_uniform_decimal_roundtrips_bit_exact() {
        let input: Vec<f64> = (0..128).map(|i| 100.0 + i as f64 * 0.1).collect();
        let got = roundtrip_f64(&input);
        for (a, b) in got.iter().zip(input.iter()) {
            assert_eq!(a.to_bits(), b.to_bits(), "mismatch at value {b}");
        }
    }

    #[test]
    fn f64_intensity_like_values_roundtrip_bit_exact() {
        let input: Vec<f64> = (0..128).map(|i| (i * i) as f64 * 12.5 + 1.0).collect();
        let got = roundtrip_f64(&input);
        for (a, b) in got.iter().zip(input.iter()) {
            assert_eq!(a.to_bits(), b.to_bits());
        }
    }

    #[test]
    fn f64_all_exceptions_fallback_to_raw_bits() {
        let input: Vec<f64> = (0..64).map(|i| f64::from_bits(0x3FF0_0000_0000_0000 + i)).collect();
        let got = roundtrip_f64(&input);
        for (a, b) in got.iter().zip(input.iter()) {
            assert_eq!(a.to_bits(), b.to_bits());
        }
    }

    #[test]
    fn f64_special_values_are_exceptions() {
        let extra = [f64::NAN, f64::INFINITY, f64::NEG_INFINITY];
        let mut input: Vec<f64> = (0..64).map(|i| 1000.0 + i as f64 * 0.5).collect();
        input.extend_from_slice(&extra);
        let got = roundtrip_f64(&input);
        for (i, (a, b)) in got.iter().zip(input.iter()).enumerate() {
            assert_eq!(a.to_bits(), b.to_bits(), "mismatch at index {i}");
        }
    }

    #[test]
    fn f64_zeros_roundtrip() {
        let input = vec![0.0f64; 64];
        let got = roundtrip_f64(&input);
        for (a, b) in got.iter().zip(input.iter()) {
            assert_eq!(a.to_bits(), b.to_bits());
        }
    }

    #[test]
    fn f32_uniform_decimal_roundtrips_bit_exact() {
        let input: Vec<f32> = (0..128).map(|i| 100.0 + i as f32 * 0.1).collect();
        let got = roundtrip_f32(&input);
        for (a, b) in got.iter().zip(input.iter()) {
            assert_eq!(a.to_bits(), b.to_bits(), "mismatch at value {b}");
        }
    }

    #[test]
    fn f32_special_values_are_exceptions() {
        let extra = [f32::NAN, f32::INFINITY, f32::NEG_INFINITY];
        let mut input: Vec<f32> = (0..64).map(|i| 500.0 + i as f32 * 2.5).collect();
        input.extend_from_slice(&extra);
        let got = roundtrip_f32(&input);
        for (i, (a, b)) in got.iter().zip(input.iter()).enumerate() {
            assert_eq!(a.to_bits(), b.to_bits(), "mismatch at index {i}");
        }
    }

    #[test]
    fn wrong_input_type_returns_error() {
        let mut out = Vec::new();
        assert!(ALP.encode(PackingInput::I32(&[1, 2, 3]), &mut out).is_err());
    }

    #[test]
    fn vbyte_roundtrip_small_values() {
        let values: Vec<i64> = (0..256).map(|i| i as i64).collect();
        let mut buf = Vec::new();
        write_delta_vbyte(&values, &mut buf).unwrap();
        let (decoded, _) = read_delta_vbyte(&buf, values.len()).unwrap();
        assert_eq!(decoded, values);
    }

    #[test]
    fn vbyte_roundtrip_negative_values() {
        let values: Vec<i64> = (-128..128).map(|i| i as i64 * 1000).collect();
        let mut buf = Vec::new();
        write_delta_vbyte(&values, &mut buf).unwrap();
        let (decoded, _) = read_delta_vbyte(&buf, values.len()).unwrap();
        assert_eq!(decoded, values);
    }

    #[test]
    fn min_input_len_is_64() {
        assert_eq!(ALP.min_input_len(), 64);
    }

    #[test]
    fn rejects_truncated_header_f64() {
        let mut out = Vec::new();
        assert!(decode_f64(&[0u8; 4], &mut out).is_err());
    }

    #[test]
    fn rejects_invalid_factor_f64() {
        let mut input = vec![MAX_FACTOR_F64 + 1];
        input.extend_from_slice(&0u32.to_le_bytes());
        input.extend_from_slice(&0u32.to_le_bytes());
        let mut out = Vec::new();
        assert!(decode_f64(&input, &mut out).is_err());
    }

    #[test]
    fn rejects_oversized_n_f64() {
        let mut input = vec![0u8];
        input.extend_from_slice(&1_000_000u32.to_le_bytes());
        input.extend_from_slice(&0u32.to_le_bytes());
        input.extend_from_slice(&0u32.to_le_bytes());
        let mut out = Vec::new();
        assert!(decode_f64(&input, &mut out).is_err());
    }

    #[test]
    fn rejects_exc_count_exceeding_n_f64() {
        let mut input = vec![0u8];
        input.extend_from_slice(&2u32.to_le_bytes());
        input.extend_from_slice(&99u32.to_le_bytes());
        input.extend_from_slice(&0u32.to_le_bytes());
        let mut out = Vec::new();
        assert!(decode_f64(&input, &mut out).is_err());
    }

    #[test]
    fn rejects_oob_exception_position_f64() {
        let vals = vec![1.0f64, 2.0, 3.0, 4.0];
        let mut enc = Vec::new();
        encode_f64(&vals, &mut enc).unwrap();
        let exc_count_pos = 5;
        enc[exc_count_pos..exc_count_pos + 4].copy_from_slice(&1u32.to_le_bytes());
        let vbyte_len = u32::from_le_bytes(enc[9..13].try_into().unwrap()) as usize;
        let exc_pos_offset = 9 + 4 + vbyte_len;
        enc.extend_from_slice(&999u32.to_le_bytes());
        enc.extend_from_slice(&0u64.to_le_bytes());
        let _ = exc_pos_offset;
        let mut out = Vec::new();
        assert!(decode_f64(&enc, &mut out).is_err());
    }

    #[test]
    fn rejects_truncated_header_f32() {
        let mut out = Vec::new();
        assert!(decode_f32(&[0u8; 4], &mut out).is_err());
    }

    #[test]
    fn rejects_invalid_factor_f32() {
        let mut input = vec![MAX_FACTOR_F32 + 1];
        input.extend_from_slice(&0u32.to_le_bytes());
        input.extend_from_slice(&0u32.to_le_bytes());
        let mut out = Vec::new();
        assert!(decode_f32(&input, &mut out).is_err());
    }

    #[test]
    fn supports_f32_and_f64_only() {
        assert!(ALP.supports(Dtype::F32));
        assert!(ALP.supports(Dtype::F64));
        assert!(!ALP.supports(Dtype::I16));
        assert!(!ALP.supports(Dtype::I32));
        assert!(!ALP.supports(Dtype::I64));
        assert!(!ALP.supports(Dtype::F16));
    }
}
