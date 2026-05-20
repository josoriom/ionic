use super::{DeltaWord, Dtype, IonResult, Packing, PackingId, PackingInput};
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
        matches!(dtype, Dtype::F32 | Dtype::F64)
    }

    fn encode(&self, input: PackingInput<'_>, out: &mut Vec<u8>) -> IonResult<()> {
        match input {
            PackingInput::F64(v) => encode::<u64>(v.iter().map(|x| x.to_bits()), v.len(), out),
            PackingInput::F32(v) => encode::<u32>(v.iter().map(|x| x.to_bits()), v.len(), out),
            _ => Err(IonError::from("Delta2VByte requires F32 or F64 input")),
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
            _ => Err(IonError::from("Delta2VByte: unsupported dtype")),
        }
    }
}

fn encode<W: DeltaWord>(
    bits: impl Iterator<Item = W>,
    len: usize,
    out: &mut Vec<u8>,
) -> IonResult<()> {
    if len == 0 {
        return Ok(());
    }
    let n = u32::try_from(len)
        .map_err(|_| IonError::from("Delta2VByte: input exceeds u32 element count"))?;
    out.extend_from_slice(&n.to_le_bytes());

    let mut iter = bits;
    let first = iter.next().unwrap();
    first.to_le_bytes_into(out);

    let second = iter.next();
    let second_delta = second
        .map(|s| DeltaWord::wrapping_sub(s, first))
        .unwrap_or_default();
    second_delta.to_le_bytes_into(out);

    let vbyte_len_pos = out.len();
    out.extend_from_slice(&[0u8; 4]);
    let vbyte_start = out.len();

    let mut prev_delta = second_delta;
    let mut prev = second.unwrap_or(first);

    for cur in iter {
        let d1 = DeltaWord::wrapping_sub(cur, prev);
        let d2 = DeltaWord::wrapping_sub(d1, prev_delta);
        write_vbyte(zigzag_encode(d2.to_signed_i64()), out);
        prev_delta = d1;
        prev = cur;
    }

    let written = u32::try_from(out.len() - vbyte_start)
        .map_err(|_| IonError::from("Delta2VByte: VByte payload exceeds 4 GiB"))?;
    out[vbyte_len_pos..vbyte_len_pos + 4].copy_from_slice(&written.to_le_bytes());
    Ok(())
}

fn decode<W: DeltaWord>(
    input: &[u8],
    out: &mut Vec<u8>,
    write_word: impl Fn(W, &mut Vec<u8>),
) -> IonResult<()> {
    let hdr = 4 + W::BYTES * 2 + 4;
    if input.len() < hdr {
        return Err(IonError::from("Delta2VByte: truncated header"));
    }

    let n = u32::from_le_bytes(input[0..4].try_into().unwrap()) as usize;
    let first = W::from_le_chunk(&input[4..4 + W::BYTES]);
    let second_delta = W::from_le_chunk(&input[4 + W::BYTES..4 + W::BYTES * 2]);
    let vbyte_len_off = 4 + W::BYTES * 2;
    let vbyte_len =
        u32::from_le_bytes(input[vbyte_len_off..vbyte_len_off + 4].try_into().unwrap()) as usize;

    if input.len() < hdr + vbyte_len {
        return Err(IonError::from("Delta2VByte: truncated VByte data"));
    }
    if n > vbyte_len + 2 {
        return Err(IonError::from(
            "Delta2VByte: n exceeds plausible bound for VByte payload",
        ));
    }

    if n >= 1 {
        write_word(first, out);
    }
    let second = if n >= 2 {
        let s = DeltaWord::wrapping_add(first, second_delta);
        write_word(s, out);
        s
    } else {
        first
    };

    let vbytes = &input[hdr..hdr + vbyte_len];
    let mut pos = 0;
    let mut prev_delta = second_delta;
    let mut prev = second;

    for _ in 2..n {
        let (z, consumed) = read_vbyte(vbytes, pos)?;
        pos += consumed;
        let d2 = W::from_u64(zigzag_decode(z) as u64);
        let d1 = DeltaWord::wrapping_add(d2, prev_delta);
        let cur = DeltaWord::wrapping_add(prev, d1);
        write_word(cur, out);
        prev_delta = d1;
        prev = cur;
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

    fn roundtrip_f64(vals: &[f64]) -> Vec<f64> {
        let mut enc = Vec::new();
        DELTA2_VBYTE.encode(PackingInput::F64(vals), &mut enc).unwrap();
        let mut dec = Vec::new();
        DELTA2_VBYTE.decode(&enc, Dtype::F64, &mut dec).unwrap();
        dec.chunks_exact(8)
            .map(|c| f64::from_le_bytes(c.try_into().unwrap()))
            .collect()
    }

    fn roundtrip_f32(vals: &[f32]) -> Vec<f32> {
        let mut enc = Vec::new();
        DELTA2_VBYTE.encode(PackingInput::F32(vals), &mut enc).unwrap();
        let mut dec = Vec::new();
        DELTA2_VBYTE.decode(&enc, Dtype::F32, &mut dec).unwrap();
        dec.chunks_exact(4)
            .map(|c| f32::from_le_bytes(c.try_into().unwrap()))
            .collect()
    }

    #[test]
    fn roundtrip_monotonic_f64() {
        let vals: Vec<f64> = (0..100).map(|i| 100.0 + i as f64 * 0.01).collect();
        let got = roundtrip_f64(&vals);
        for (a, b) in got.iter().zip(vals.iter()) {
            assert_eq!(a.to_bits(), b.to_bits());
        }
    }

    #[test]
    fn roundtrip_monotonic_f32() {
        let vals: Vec<f32> = (0..100).map(|i| 100.0 + i as f32 * 0.01).collect();
        let got = roundtrip_f32(&vals);
        for (a, b) in got.iter().zip(vals.iter()) {
            assert_eq!(a.to_bits(), b.to_bits());
        }
    }

    #[test]
    fn roundtrip_single_f64() {
        let vals = vec![42.0f64, 43.5, 100.0];
        let got = roundtrip_f64(&vals);
        for (a, b) in got.iter().zip(vals.iter()) {
            assert_eq!(a.to_bits(), b.to_bits());
        }
    }

    #[test]
    fn roundtrip_single_f32() {
        let vals = vec![42.0f32, 43.5, 100.0];
        let got = roundtrip_f32(&vals);
        for (a, b) in got.iter().zip(vals.iter()) {
            assert_eq!(a.to_bits(), b.to_bits());
        }
    }

    #[test]
    fn header_size_f32_smaller_than_f64() {
        let vals: Vec<f32> = (0..10).map(|i| i as f32).collect();
        let mut enc32 = Vec::new();
        DELTA2_VBYTE.encode(PackingInput::F32(&vals), &mut enc32).unwrap();

        let vals64: Vec<f64> = (0..10).map(|i| i as f64).collect();
        let mut enc64 = Vec::new();
        DELTA2_VBYTE.encode(PackingInput::F64(&vals64), &mut enc64).unwrap();

        assert!(enc32.len() < enc64.len());
    }
}
