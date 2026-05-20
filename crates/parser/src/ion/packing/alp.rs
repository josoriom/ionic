use vortex_alp::{ALPFloat, Exponents};

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
        32
    }

    fn supports(&self, dtype: Dtype) -> bool {
        matches!(dtype, Dtype::F32 | Dtype::F64)
    }

    fn encode(&self, input: PackingInput<'_>, out: &mut Vec<u8>) -> IonResult<()> {
        match input {
            PackingInput::F64(v) => encode_f64(v, out),
            PackingInput::F32(v) => encode_f32(v, out),
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

// Wire format (shared by f32 and f64):
//   e:           u8  (1 byte)   — ALP exponent e
//   f:           u8  (1 byte)   — ALP exponent f
//   n:           u32 (4 bytes)  — element count
//   patch_count: u32 (4 bytes)  — number of exception patches
//   encoded_ints n * sizeof(ALPInt) bytes  (i64 for f64, i32 for f32)
//   patch_indices patch_count * 4 bytes   (u32 LE, index into encoded array)
//   patch_values  patch_count * sizeof(T) bytes  (raw LE bytes of the original float)
const HDR: usize = 10; // e(1) + f(1) + n(4) + patch_count(4)

fn encode_f64(values: &[f64], out: &mut Vec<u8>) -> IonResult<()> {
    let n = u32::try_from(values.len())
        .map_err(|_| IonError::from("ALP F64: input exceeds u32 element count"))?;

    let (exp, encoded, patch_idx, patch_val, _chunk_offsets) = f64::encode(values, None);
    let patch_count = u32::try_from(patch_idx.len())
        .map_err(|_| IonError::from("ALP F64: patch count exceeds u32"))?;

    out.push(exp.e);
    out.push(exp.f);
    out.extend_from_slice(&n.to_le_bytes());
    out.extend_from_slice(&patch_count.to_le_bytes());
    for &v in encoded.iter() {
        out.extend_from_slice(&v.to_le_bytes());
    }
    for &idx in patch_idx.iter() {
        out.extend_from_slice(&(idx as u32).to_le_bytes());
    }
    for &v in patch_val.iter() {
        out.extend_from_slice(&v.to_le_bytes());
    }
    Ok(())
}

fn decode_f64(input: &[u8], out: &mut Vec<u8>) -> IonResult<()> {
    if input.len() < HDR {
        return Err(IonError::from("ALP F64: truncated header"));
    }
    let exp = Exponents { e: input[0], f: input[1] };
    let n = u32::from_le_bytes(input[2..6].try_into().unwrap()) as usize;
    let patch_count = u32::from_le_bytes(input[6..10].try_into().unwrap()) as usize;

    let encoded_end = HDR
        .checked_add(n.checked_mul(8).ok_or_else(|| IonError::from("ALP F64: encoded size overflow"))?)
        .ok_or_else(|| IonError::from("ALP F64: encoded offset overflow"))?;
    let patch_idx_end = encoded_end
        .checked_add(patch_count.checked_mul(4).ok_or_else(|| IonError::from("ALP F64: patch index overflow"))?)
        .ok_or_else(|| IonError::from("ALP F64: patch index offset overflow"))?;
    let patch_val_end = patch_idx_end
        .checked_add(patch_count.checked_mul(8).ok_or_else(|| IonError::from("ALP F64: patch value overflow"))?)
        .ok_or_else(|| IonError::from("ALP F64: patch value offset overflow"))?;

    if input.len() < patch_val_end {
        return Err(IonError::from("ALP F64: truncated data"));
    }

    let encoded: Vec<i64> = input[HDR..encoded_end]
        .chunks_exact(8)
        .map(|c| i64::from_le_bytes(c.try_into().unwrap()))
        .collect();

    let mut values = vec![0.0f64; n];
    f64::decode_into(&encoded, exp, &mut values);

    for i in 0..patch_count {
        let idx = u32::from_le_bytes(
            input[encoded_end + i * 4..encoded_end + i * 4 + 4].try_into().unwrap(),
        ) as usize;
        if idx >= n {
            return Err(IonError::from("ALP F64: patch index out of bounds"));
        }
        values[idx] = f64::from_le_bytes(
            input[patch_idx_end + i * 8..patch_idx_end + i * 8 + 8].try_into().unwrap(),
        );
    }

    out.reserve(n * 8);
    for v in values {
        out.extend_from_slice(&v.to_le_bytes());
    }
    Ok(())
}

fn encode_f32(values: &[f32], out: &mut Vec<u8>) -> IonResult<()> {
    let n = u32::try_from(values.len())
        .map_err(|_| IonError::from("ALP F32: input exceeds u32 element count"))?;

    let (exp, encoded, patch_idx, patch_val, _chunk_offsets) = f32::encode(values, None);
    let patch_count = u32::try_from(patch_idx.len())
        .map_err(|_| IonError::from("ALP F32: patch count exceeds u32"))?;

    out.push(exp.e);
    out.push(exp.f);
    out.extend_from_slice(&n.to_le_bytes());
    out.extend_from_slice(&patch_count.to_le_bytes());
    for &v in encoded.iter() {
        out.extend_from_slice(&v.to_le_bytes());
    }
    for &idx in patch_idx.iter() {
        out.extend_from_slice(&(idx as u32).to_le_bytes());
    }
    for &v in patch_val.iter() {
        out.extend_from_slice(&v.to_le_bytes());
    }
    Ok(())
}

fn decode_f32(input: &[u8], out: &mut Vec<u8>) -> IonResult<()> {
    if input.len() < HDR {
        return Err(IonError::from("ALP F32: truncated header"));
    }
    let exp = Exponents { e: input[0], f: input[1] };
    let n = u32::from_le_bytes(input[2..6].try_into().unwrap()) as usize;
    let patch_count = u32::from_le_bytes(input[6..10].try_into().unwrap()) as usize;

    let encoded_end = HDR
        .checked_add(n.checked_mul(4).ok_or_else(|| IonError::from("ALP F32: encoded size overflow"))?)
        .ok_or_else(|| IonError::from("ALP F32: encoded offset overflow"))?;
    let patch_idx_end = encoded_end
        .checked_add(patch_count.checked_mul(4).ok_or_else(|| IonError::from("ALP F32: patch index overflow"))?)
        .ok_or_else(|| IonError::from("ALP F32: patch index offset overflow"))?;
    let patch_val_end = patch_idx_end
        .checked_add(patch_count.checked_mul(4).ok_or_else(|| IonError::from("ALP F32: patch value overflow"))?)
        .ok_or_else(|| IonError::from("ALP F32: patch value offset overflow"))?;

    if input.len() < patch_val_end {
        return Err(IonError::from("ALP F32: truncated data"));
    }

    let encoded: Vec<i32> = input[HDR..encoded_end]
        .chunks_exact(4)
        .map(|c| i32::from_le_bytes(c.try_into().unwrap()))
        .collect();

    let mut values = vec![0.0f32; n];
    f32::decode_into(&encoded, exp, &mut values);

    for i in 0..patch_count {
        let idx = u32::from_le_bytes(
            input[encoded_end + i * 4..encoded_end + i * 4 + 4].try_into().unwrap(),
        ) as usize;
        if idx >= n {
            return Err(IonError::from("ALP F32: patch index out of bounds"));
        }
        values[idx] = f32::from_le_bytes(
            input[patch_idx_end + i * 4..patch_idx_end + i * 4 + 4].try_into().unwrap(),
        );
    }

    out.reserve(n * 4);
    for v in values {
        out.extend_from_slice(&v.to_le_bytes());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip_f64(vals: &[f64]) -> Vec<f64> {
        let mut enc = Vec::new();
        ALP.encode(PackingInput::F64(vals), &mut enc).unwrap();
        let mut dec = Vec::new();
        ALP.decode(&enc, Dtype::F64, &mut dec).unwrap();
        dec.chunks_exact(8).map(|c| f64::from_le_bytes(c.try_into().unwrap())).collect()
    }

    fn roundtrip_f32(vals: &[f32]) -> Vec<f32> {
        let mut enc = Vec::new();
        ALP.encode(PackingInput::F32(vals), &mut enc).unwrap();
        let mut dec = Vec::new();
        ALP.decode(&enc, Dtype::F32, &mut dec).unwrap();
        dec.chunks_exact(4).map(|c| f32::from_le_bytes(c.try_into().unwrap())).collect()
    }

    #[test]
    fn decimal_f64_roundtrips_bit_exact() {
        let vals: Vec<f64> = (0..128).map(|i| 100.0 + i as f64 * 0.1).collect();
        let got = roundtrip_f64(&vals);
        for (a, b) in got.iter().zip(vals.iter()) {
            assert_eq!(a.to_bits(), b.to_bits());
        }
    }

    #[test]
    fn decimal_f32_roundtrips_bit_exact() {
        let vals: Vec<f32> = (0..128).map(|i| 100.0 + i as f32 * 0.1).collect();
        let got = roundtrip_f32(&vals);
        for (a, b) in got.iter().zip(vals.iter()) {
            assert_eq!(a.to_bits(), b.to_bits());
        }
    }

    #[test]
    fn special_values_f64_roundtrip_via_patches() {
        let mut vals: Vec<f64> = (0..64).map(|i| 500.0 + i as f64 * 1.5).collect();
        vals.extend_from_slice(&[f64::NAN, f64::INFINITY, f64::NEG_INFINITY]);
        let got = roundtrip_f64(&vals);
        for (a, b) in got.iter().zip(vals.iter()) {
            assert_eq!(a.to_bits(), b.to_bits());
        }
    }

    #[test]
    fn special_values_f32_roundtrip_via_patches() {
        let mut vals: Vec<f32> = (0..64).map(|i| 500.0 + i as f32 * 1.5).collect();
        vals.extend_from_slice(&[f32::NAN, f32::INFINITY, f32::NEG_INFINITY]);
        let got = roundtrip_f32(&vals);
        for (a, b) in got.iter().zip(vals.iter()) {
            assert_eq!(a.to_bits(), b.to_bits());
        }
    }

    #[test]
    fn wrong_input_type_returns_error() {
        let mut out = Vec::new();
        assert!(ALP.encode(PackingInput::I32(&[1, 2, 3]), &mut out).is_err());
    }

    #[test]
    fn rejects_truncated_header() {
        let mut out = Vec::new();
        assert!(ALP.decode(&[0u8; 4], Dtype::F64, &mut out).is_err());
        assert!(ALP.decode(&[0u8; 4], Dtype::F32, &mut out).is_err());
    }
}
