use super::{Dtype, IonResult, Packing, PackingId, PackingInput};
use crate::ion::IonError;

pub(crate) static DELTA2_VBYTE: Delta2VByte = Delta2VByte;
pub(crate) struct Delta2VByte;

impl Packing for Delta2VByte {
    fn id(&self) -> PackingId {
        PackingId::DeltaSquaredVByte
    }

    fn is_variable_length(&self) -> bool {
        true
    }

    fn min_input_len(&self) -> usize {
        3
    }

    fn supports(&self, dtype: Dtype) -> bool {
        matches!(dtype, Dtype::F64)
    }

    fn encode(&self, input: PackingInput<'_>, out: &mut Vec<u8>) -> IonResult<()> {
        match input {
            PackingInput::F64(values) => encode_f64(values, out),
            _ => Err(IonError::from("Delta2VByte requires F64 input")),
        }
    }

    fn decode(&self, input: &[u8], dtype: Dtype, out: &mut Vec<u8>) -> IonResult<()> {
        match dtype {
            Dtype::F64 => decode_f64(input, out),
            _ => Err(IonError::from("Delta2VByte: unsupported dtype")),
        }
    }
}

fn encode_f64(values: &[f64], out: &mut Vec<u8>) -> IonResult<()> {
    if values.is_empty() {
        return Ok(());
    }
    let n = u32::try_from(values.len())
        .map_err(|_| IonError::from("Delta2VByte: input exceeds u32 element count"))?;
    out.extend_from_slice(&n.to_le_bytes());

    let bits: Vec<i64> = values.iter().map(|v| v.to_bits() as i64).collect();

    let first = bits[0];
    let second_delta = if bits.len() > 1 { bits[1].wrapping_sub(first) } else { 0i64 };
    out.extend_from_slice(&first.to_le_bytes());
    out.extend_from_slice(&second_delta.to_le_bytes());

    let vbyte_len_pos = out.len();
    out.extend_from_slice(&[0u8; 4]);
    let vbyte_start = out.len();

    let mut prev_delta = second_delta;
    let mut prev = if bits.len() > 1 { bits[1] } else { first };
    for &cur in bits.iter().skip(2) {
        let d1 = cur.wrapping_sub(prev);
        let d2 = d1.wrapping_sub(prev_delta);
        let z = zigzag_encode(d2);
        write_vbyte(z, out);
        prev_delta = d1;
        prev = cur;
    }

    let written = u32::try_from(out.len() - vbyte_start)
        .map_err(|_| IonError::from("Delta2VByte: VByte payload exceeds 4 GiB"))?;
    out[vbyte_len_pos..vbyte_len_pos + 4].copy_from_slice(&written.to_le_bytes());
    Ok(())
}

fn decode_f64(input: &[u8], out: &mut Vec<u8>) -> IonResult<()> {
    if input.len() < 20 {
        return Err(IonError::from("Delta2VByte: truncated header"));
    }
    let n = u32::from_le_bytes(input[0..4].try_into().unwrap()) as usize;
    let first = i64::from_le_bytes(input[4..12].try_into().unwrap());
    let second_delta = i64::from_le_bytes(input[12..20].try_into().unwrap());
    let vbyte_len = u32::from_le_bytes(input[20..24].try_into().unwrap()) as usize;

    if input.len() < 24 + vbyte_len {
        return Err(IonError::from("Delta2VByte: truncated VByte data"));
    }

    if n > vbyte_len + 2 {
        return Err(IonError::from("Delta2VByte: n exceeds plausible bound for VByte payload"));
    }
    let mut values: Vec<i64> = Vec::with_capacity(n);
    if n >= 1 {
        values.push(first);
    }
    if n >= 2 {
        values.push(first.wrapping_add(second_delta));
    }

    let vbytes = &input[24..24 + vbyte_len];
    let mut pos = 0;
    let mut prev_delta = second_delta;
    let mut prev = if n >= 2 { first.wrapping_add(second_delta) } else { first };

    for _ in 2..n {
        let (z, consumed) = read_vbyte(vbytes, pos)?;
        pos += consumed;
        let d2 = zigzag_decode(z);
        let d1 = d2.wrapping_add(prev_delta);
        let cur = prev.wrapping_add(d1);
        values.push(cur);
        prev_delta = d1;
        prev = cur;
    }

    for bits in values {
        out.extend_from_slice(&f64::from_bits(bits as u64).to_le_bytes());
    }
    Ok(())
}

#[inline]
fn zigzag_encode(v: i64) -> u64 {
    ((v << 1) ^ (v >> 63)) as u64
}

#[inline]
fn zigzag_decode(v: u64) -> i64 {
    ((v >> 1) as i64) ^ (-((v & 1) as i64))
}

#[inline]
fn write_vbyte(mut v: u64, out: &mut Vec<u8>) {
    loop {
        let b = (v & 0x7F) as u8;
        v >>= 7;
        if v == 0 {
            out.push(b);
            break;
        }
        out.push(b | 0x80);
    }
}

fn read_vbyte(buf: &[u8], pos: usize) -> IonResult<(u64, usize)> {
    let mut result = 0u64;
    let mut shift = 0u32;
    let mut i = pos;
    loop {
        if i >= buf.len() {
            return Err(IonError::from("Delta2VByte: VByte overflow"));
        }
        let b = buf[i];
        i += 1;
        result |= ((b & 0x7F) as u64) << shift;
        if b & 0x80 == 0 {
            break;
        }
        shift += 7;
        if shift >= 64 {
            return Err(IonError::from("Delta2VByte: VByte too wide"));
        }
    }
    Ok((result, i - pos))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_monotonic() {
        let vals: Vec<f64> = (0..100).map(|i| 100.0 + i as f64 * 0.01).collect();
        let mut enc = Vec::new();
        encode_f64(&vals, &mut enc).unwrap();
        let mut dec = Vec::new();
        decode_f64(&enc, &mut dec).unwrap();
        let got: Vec<f64> = dec.chunks_exact(8).map(|c| f64::from_le_bytes(c.try_into().unwrap())).collect();
        for (a, b) in got.iter().zip(vals.iter()) {
            assert_eq!(a.to_bits(), b.to_bits());
        }
    }

    #[test]
    fn roundtrip_single() {
        let vals = vec![42.0f64, 43.5, 100.0];
        let mut enc = Vec::new();
        encode_f64(&vals, &mut enc).unwrap();
        let mut dec = Vec::new();
        decode_f64(&enc, &mut dec).unwrap();
        let got: Vec<f64> = dec.chunks_exact(8).map(|c| f64::from_le_bytes(c.try_into().unwrap())).collect();
        for (a, b) in got.iter().zip(vals.iter()) {
            assert_eq!(a.to_bits(), b.to_bits());
        }
    }
}
