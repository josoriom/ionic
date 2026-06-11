#[cfg(all(target_arch = "wasm32", not(target_os = "wasi")))]
use std::io::Read;
#[cfg(not(all(target_arch = "wasm32", not(target_os = "wasi"))))]
use std::mem::MaybeUninit;
#[cfg(not(all(target_arch = "wasm32", not(target_os = "wasi"))))]
use zstd::zstd_safe;

use crate::{
    decoder::{
        decode::{Metadatum, MetadatumValue},
        utilities::decompression_limit::DecompressionLimit,
    },
    ion::{
        IonError, IonResult,
        attr_meta::{AccessionTail, CV_CODE_UNKNOWN, cv_ref_code_from_str},
    },
    mzml::schema::{SchemaNode, SchemaTree as Schema, TagId},
    mzml::structs::{BinaryData, BinaryDataArray, BinaryDataArrayList},
};

#[allow(dead_code)]
pub(crate) const ACC_Y_INTENSITY: &str = "MS:1000515";
#[allow(dead_code)]
pub(crate) const ACC_Y_SNR: &str = "MS:1000786";

#[inline]
pub(crate) fn take<'a>(
    bytes: &'a [u8],
    pos: &mut usize,
    n: usize,
    field: &'static str,
) -> IonResult<&'a [u8]> {
    let start = *pos;
    let end = start
        .checked_add(n)
        .ok_or_else(|| IonError::from(format!("overflow while reading {field}")))?;
    if end > bytes.len() {
        return Err(format!(
            "unexpected EOF while reading {field}: need {n} bytes at pos {pos}, len {}",
            bytes.len()
        )
        .into());
    }
    *pos = end;
    Ok(&bytes[start..end])
}

#[inline]
pub(crate) fn read_u16_le_at(bytes: &[u8], pos: &mut usize, ctx: &str) -> IonResult<u16> {
    if *pos + 2 > bytes.len() {
        return Err(format!("{ctx}: not enough bytes for u16 at offset {pos}").into());
    }
    let v = u16::from_le_bytes(bytes[*pos..*pos + 2].try_into().unwrap());
    *pos += 2;
    Ok(v)
}

#[inline]
pub(crate) fn read_u32_le_at(bytes: &[u8], pos: &mut usize, field: &'static str) -> IonResult<u32> {
    let s = take(bytes, pos, 4, field)?;
    Ok(u32::from_le_bytes(s.try_into().unwrap()))
}

#[inline]
pub(crate) fn read_u64_le_at(bytes: &[u8], pos: &mut usize, field: &'static str) -> IonResult<u64> {
    let s = take(bytes, pos, 8, field)?;
    Ok(u64::from_le_bytes(s.try_into().unwrap()))
}

#[inline]
pub(crate) fn read_u32_vec(bytes: &[u8], pos: &mut usize, n: usize) -> IonResult<Vec<u32>> {
    let byte_len = n
        .checked_mul(4)
        .ok_or_else(|| IonError::from("u32 vector length overflows usize"))?;
    let raw = take(bytes, pos, byte_len, "u32 vector")?;
    let mut out = Vec::with_capacity(n);
    for chunk in raw.chunks_exact(4) {
        out.push(u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
    }
    Ok(out)
}

#[inline]
pub(crate) fn read_f64_vec(bytes: &[u8], pos: &mut usize, n: usize) -> IonResult<Vec<f64>> {
    let byte_len = n
        .checked_mul(8)
        .ok_or_else(|| IonError::from("f64 vector length overflows usize"))?;
    let raw = take(bytes, pos, byte_len, "f64 vector")?;
    let mut out = Vec::with_capacity(n);
    for chunk in raw.chunks_exact(8) {
        out.push(f64::from_le_bytes(chunk.try_into().unwrap()));
    }
    Ok(out)
}

#[inline]
#[cfg(not(all(target_arch = "wasm32", not(target_os = "wasi"))))]
pub(crate) fn decompress_zstd(
    comp: &[u8],
    expected: usize,
    budget: DecompressionLimit,
) -> IonResult<Vec<u8>> {
    if expected == 0 {
        return Ok(Vec::new());
    }

    budget.validate(comp.len(), expected)?;

    let mut out: Vec<u8> = Vec::with_capacity(expected);

    let actual = {
        let spare: &mut [MaybeUninit<u8>] = out.spare_capacity_mut();
        let dst =
            unsafe { std::slice::from_raw_parts_mut(spare.as_mut_ptr() as *mut u8, expected) };

        zstd_safe::decompress(dst, comp)
            .map_err(|e| IonError::from(format!("zstd decode failed: {e:?}")))?
    };

    if actual != expected {
        return Err(format!("zstd: bad decoded size (got={actual}, expected={expected})").into());
    }

    unsafe {
        out.set_len(actual);
    }

    Ok(out)
}

#[inline]
#[cfg(all(target_arch = "wasm32", not(target_os = "wasi")))]
pub(crate) fn decompress_zstd(
    comp: &[u8],
    expected: usize,
    budget: DecompressionLimit,
) -> IonResult<Vec<u8>> {
    if expected == 0 {
        return Ok(Vec::new());
    }

    budget.validate(comp.len(), expected)?;

    let mut decoder = ruzstd::StreamingDecoder::new(comp)
        .map_err(|err| IonError::from(format!("zstd decode failed: {err:?}")))?;
    let mut out = Vec::with_capacity(expected);
    decoder
        .read_to_end(&mut out)
        .map_err(|err| IonError::from(format!("zstd decode failed: {err}")))?;

    if out.len() != expected {
        return Err(format!(
            "zstd: bad decoded size (got={}, expected={expected})",
            out.len()
        )
        .into());
    }

    Ok(out)
}

#[inline]
pub(crate) fn decompress_zstd_allow_aligned_padding(
    input: &[u8],
    expected: usize,
    budget: DecompressionLimit,
) -> IonResult<Vec<u8>> {
    if expected == 0 {
        return Ok(Vec::new());
    }

    #[cfg(not(all(target_arch = "wasm32", not(target_os = "wasi"))))]
    if let Ok(n) = zstd_safe::find_frame_compressed_size(input)
        && n > 0
        && n <= input.len()
        && let Ok(v) = decompress_zstd(&input[..n], expected, budget)
    {
        return Ok(v);
    }

    match decompress_zstd(input, expected, budget) {
        Ok(v) => Ok(v),
        Err(first_err) => {
            let mut trimmed = input;
            for _ in 0..7 {
                let Some(&last) = trimmed.last() else { break };
                if last != 0 {
                    break;
                }
                trimmed = &trimmed[..trimmed.len() - 1];
                if let Ok(v) = decompress_zstd(trimmed, expected, budget) {
                    return Ok(v);
                }
            }
            Err(first_err)
        }
    }
}

#[inline]
pub(crate) fn is_cv_prefix(p: &str) -> bool {
    cv_ref_code_from_str(Some(p)) != CV_CODE_UNKNOWN
}

#[inline]
pub(crate) fn unit_cv_ref(unit_accession: Option<&str>) -> Option<String> {
    unit_accession
        .and_then(|ua| ua.split_once(':'))
        .map(|(pref, _)| pref.to_owned())
}

#[inline]
pub(crate) fn value_to_opt_string(v: &MetadatumValue) -> Option<String> {
    match v {
        MetadatumValue::Number(x) => Some(x.to_string()),
        MetadatumValue::Text(s) => Some(s.clone()),
        MetadatumValue::Empty => None,
    }
}

#[inline]
pub(crate) fn get_attr_u32(rows: &[&Metadatum], tail: AccessionTail) -> Option<u32> {
    for m in rows {
        if let Some(acc) = m.accession.as_deref()
            && parse_accession_tail(Some(acc)) == tail
        {
            return match &m.value {
                MetadatumValue::Number(n)
                    if n.is_finite() && *n >= 0.0 && *n <= u32::MAX as f64 =>
                {
                    Some(*n as u32)
                }
                MetadatumValue::Text(s) => s.parse::<u32>().ok(),
                _ => None,
            };
        }
    }
    None
}

#[inline]
pub(crate) fn get_attr_text(rows: &[&Metadatum], tail: AccessionTail) -> Option<String> {
    for m in rows {
        if let Some(acc) = m.accession.as_deref()
            && parse_accession_tail(Some(acc)) == tail
        {
            return match &m.value {
                MetadatumValue::Text(s) => Some(s.clone()),
                MetadatumValue::Number(n) => Some(n.to_string()),
                MetadatumValue::Empty => None,
            };
        }
    }
    None
}

#[inline]
pub(crate) fn vs_len_bytes(vk: &[u8], vi: &[u32], voff: &[u32], vlen: &[u32]) -> IonResult<usize> {
    let mut max_end = 0usize;
    for (j, &kind) in vk.iter().enumerate() {
        if kind != 1 {
            continue;
        }
        let idx = vi[j] as usize;
        if idx >= voff.len() || idx >= vlen.len() {
            return Err("string value index out of range".into());
        }
        let end = (voff[idx] as usize)
            .checked_add(vlen[idx] as usize)
            .ok_or_else(|| IonError::from("string offset+length overflow"))?;
        if end > max_end {
            max_end = end;
        }
    }
    Ok(max_end)
}

#[allow(dead_code)]
#[inline]
pub(crate) fn xy_lengths_from_bdal(
    list: Option<&BinaryDataArrayList>,
) -> (Option<usize>, Option<usize>) {
    let Some(list) = list else {
        return (None, None);
    };
    let (mut x_len, mut y_len) = (None, None);
    for bda in &list.binary_data_arrays {
        let len = decoded_len(bda);
        if len == 0 {
            continue;
        }
        if is_y_array(bda) {
            y_len.get_or_insert(len);
        } else {
            x_len.get_or_insert(len);
        }
        if x_len.is_some() && y_len.is_some() {
            break;
        }
    }
    (x_len, y_len)
}

#[allow(dead_code)]
#[inline]
pub(crate) fn decoded_len(bda: &BinaryDataArray) -> usize {
    match bda.binary.as_ref() {
        None => 0,
        Some(bin) => match bin {
            BinaryData::F16(v) => v.len(),
            BinaryData::F32(v) => v.len(),
            BinaryData::F64(v) => v.len(),
            BinaryData::I16(v) => v.len(),
            BinaryData::I32(v) => v.len(),
            BinaryData::I64(v) => v.len(),
        },
    }
}

#[allow(dead_code)]
#[inline]
pub(crate) fn is_y_array(bda: &BinaryDataArray) -> bool {
    bda.cv_params.iter().any(|p| {
        matches!(
            p.accession.as_deref(),
            Some(ACC_Y_INTENSITY) | Some(ACC_Y_SNR)
        )
    })
}

#[allow(dead_code)]
#[inline]
pub(crate) fn find_node_by_tag(schema: &Schema, tag: TagId) -> Option<&SchemaNode> {
    if let Some(n) = schema.root_by_tag(tag) {
        return Some(n);
    }
    for root in schema.roots.values() {
        let mut stack = vec![root];
        while let Some(node) = stack.pop() {
            if node.self_tags.contains(&tag) {
                return Some(node);
            }
            stack.extend(node.children.values());
        }
    }
    None
}

#[inline]
pub(crate) fn parse_accession_tail(accession: Option<&str>) -> AccessionTail {
    let s = accession.unwrap_or("");
    let tail = s.rsplit_once(':').map(|(_, t)| t).unwrap_or(s);
    let mut v: u32 = 0;
    let mut saw = false;
    for b in tail.bytes() {
        if b.is_ascii_digit() {
            saw = true;
            v = match v
                .checked_mul(10)
                .and_then(|x| x.checked_add((b - b'0') as u32))
            {
                Some(n) => n,
                None => return AccessionTail::from_raw(0),
            };
        }
    }
    AccessionTail::from_raw(if saw { v } else { 0 })
}
