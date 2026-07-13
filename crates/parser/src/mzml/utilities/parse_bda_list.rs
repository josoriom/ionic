use std::io::BufRead;

use base64::{Engine, engine::general_purpose::STANDARD};
use miniz_oxide::inflate::decompress_to_vec_zlib_with_limit;
use quick_xml::events::BytesStart;

use crate::{
    accessions::{
        ACC_COMPRESSION_ZLIB, ACC_FLOAT_16BIT_STR, ACC_FLOAT_32BIT_STR, ACC_FLOAT_64BIT_STR,
        ACC_INT_16BIT_STR, ACC_INT_32BIT_STR, ACC_INT_64BIT_STR,
    },
    mzml::{
        schema::TagId,
        structs::{BinaryDataArray, BinaryDataArrayList, NumericArray, NumericType},
        utilities::{
            PREALLOC_CAP, ParamCollector, ParseError, ParsingWorkspace, attr, attr_usize,
            read_base64_binary, read_cv_param, read_ref_group_ref, read_user_param,
        },
    },
};

pub(crate) fn parse_bda_list<R: BufRead>(
    ws: &mut ParsingWorkspace<R>,
    start: &BytesStart<'_>,
) -> Result<BinaryDataArrayList, ParseError> {
    let count = attr_usize(start, b"count");
    let mut list = BinaryDataArrayList {
        count,
        binary_data_arrays: Vec::with_capacity(count.unwrap_or(0).min(PREALLOC_CAP)),
    };
    ws.for_each_child(start, |ws, event| {
        let (tag, element, is_open) = event.into_parts();
        if tag != TagId::BinaryDataArray {
            return Ok(false);
        }
        if is_open {
            list.binary_data_arrays.push(parse_bda(ws, &element)?);
        } else {
            list.binary_data_arrays.push(BinaryDataArray {
                array_length: attr_usize(&element, b"arrayLength"),
                encoded_length: attr_usize(&element, b"encodedLength"),
                data_processing_ref: attr(&element, b"dataProcessingRef"),
                ..Default::default()
            });
        }
        Ok(true)
    })?;
    Ok(list)
}

pub(crate) fn parse_bda<R: BufRead>(
    ws: &mut ParsingWorkspace<R>,
    start: &BytesStart<'_>,
) -> Result<BinaryDataArray, ParseError> {
    let mut bda = BinaryDataArray {
        array_length: attr_usize(start, b"arrayLength"),
        encoded_length: attr_usize(start, b"encodedLength"),
        data_processing_ref: attr(start, b"dataProcessingRef"),
        ..Default::default()
    };
    let mut base64_bytes: Vec<u8> = Vec::new();

    ws.for_each_child(start, |ws, event| {
        let (tag, element, _) = event.into_parts();
        match tag {
            TagId::CvParam => {
                bda.receive_cv(read_cv_param(&element));
                Ok(true)
            }
            TagId::UserParam => {
                bda.receive_user(read_user_param(&element));
                Ok(true)
            }
            TagId::ReferenceableParamGroupRef => {
                bda.receive_ref_group(read_ref_group_ref(&element));
                Ok(true)
            }
            TagId::Binary => {
                if let Some(len) = bda.encoded_length {
                    base64_bytes.reserve(len.min(PREALLOC_CAP));
                }
                let closing = element.name().as_ref().to_vec();
                read_base64_binary(ws, &closing, &mut base64_bytes)?;
                Ok(true)
            }
            _ => Ok(false),
        }
    })?;

    let encoding = encoding_for_array(&bda);
    bda.numeric_type = Some(encoding.numeric_type);

    if !base64_bytes.is_empty() {
        bda.pending_zlib = encoding.is_zlib_compressed;
        bda.pending_base64 = Some(base64_bytes);
    }

    Ok(bda)
}

const MAX_NUMERIC_STRIDE_BYTES: usize = 8;
const DECOMPRESSED_SIZE_MARGIN_BYTES: usize = 1024;
const MAX_UNCOMPRESSED_SIZE_WITHOUT_DECLARED_LENGTH: usize = 1 << 31;

fn max_decompressed_size(array_length: Option<usize>) -> usize {
    match array_length {
        Some(declared_length) => declared_length
            .saturating_mul(MAX_NUMERIC_STRIDE_BYTES)
            .saturating_add(DECOMPRESSED_SIZE_MARGIN_BYTES),
        None => MAX_UNCOMPRESSED_SIZE_WITHOUT_DECLARED_LENGTH,
    }
}

pub(crate) fn finalize_bda(
    bda: &mut BinaryDataArray,
    default_array_length: Option<usize>,
) -> Result<(), ParseError> {
    let Some(base64_bytes) = bda.pending_base64.take() else {
        return Ok(());
    };
    let mut decoded = Vec::with_capacity(base64_bytes.len() / 4 * 3 + 8);
    STANDARD.decode_vec(&base64_bytes, &mut decoded)?;
    if bda.pending_zlib {
        let declared_length = bda.array_length.or(default_array_length);
        let max_size = max_decompressed_size(declared_length);
        decoded = decompress_to_vec_zlib_with_limit(&decoded, max_size)
            .map_err(|e| ParseError::Decompress(format!("{e:?}")))?;
    }
    bda.binary = Some(decode_binary_data(
        bda.numeric_type.unwrap_or(NumericType::Float64),
        &decoded,
        bda.array_length,
    ));
    Ok(())
}

#[derive(Debug, Clone, Copy)]
struct BinaryArrayEncoding {
    is_zlib_compressed: bool,
    numeric_type: NumericType,
}

fn encoding_for_array(bda: &BinaryDataArray) -> BinaryArrayEncoding {
    let has = |acc: &str| {
        bda.cv_params
            .iter()
            .any(|p| p.accession.as_deref() == Some(acc))
    };
    let is_zlib_compressed = has(ACC_COMPRESSION_ZLIB);
    let (f64, f32, f16) = (
        has(ACC_FLOAT_64BIT_STR),
        has(ACC_FLOAT_32BIT_STR),
        has(ACC_FLOAT_16BIT_STR),
    );
    let (i64, i32, i16) = (
        has(ACC_INT_64BIT_STR),
        has(ACC_INT_32BIT_STR),
        has(ACC_INT_16BIT_STR),
    );

    let numeric_type = if let Some(declared) = bda.numeric_type {
        declared
    } else if f16 && !f32 && !f64 {
        NumericType::Float16
    } else if i16 && !i32 && !i64 {
        NumericType::Int16
    } else if i32 && !i64 {
        NumericType::Int32
    } else if i64 && !f64 && !f32 {
        NumericType::Int64
    } else if f64 {
        NumericType::Float64
    } else if f32 {
        NumericType::Float32
    } else if i64 {
        NumericType::Int64
    } else {
        NumericType::Float64
    };

    BinaryArrayEncoding {
        is_zlib_compressed,
        numeric_type,
    }
}

fn decode_binary_data(
    numeric_type: NumericType,
    decoded: &[u8],
    array_length: Option<usize>,
) -> NumericArray {
    match numeric_type {
        NumericType::Float64 => {
            NumericArray::F64(decode_packed_numeric_bytes(decoded, 8, array_length, |c| {
                f64::from_le_bytes(c.try_into().unwrap())
            }))
        }
        NumericType::Float32 => {
            NumericArray::F32(decode_packed_numeric_bytes(decoded, 4, array_length, |c| {
                f32::from_le_bytes(c.try_into().unwrap())
            }))
        }
        NumericType::Float16 => {
            NumericArray::F16(decode_packed_numeric_bytes(decoded, 2, array_length, |c| {
                u16::from_le_bytes(c.try_into().unwrap())
            }))
        }
        NumericType::Int64 => {
            NumericArray::I64(decode_packed_numeric_bytes(decoded, 8, array_length, |c| {
                i64::from_le_bytes(c.try_into().unwrap())
            }))
        }
        NumericType::Int32 => {
            NumericArray::I32(decode_packed_numeric_bytes(decoded, 4, array_length, |c| {
                i32::from_le_bytes(c.try_into().unwrap())
            }))
        }
        NumericType::Int16 => {
            NumericArray::I16(decode_packed_numeric_bytes(decoded, 2, array_length, |c| {
                i16::from_le_bytes(c.try_into().unwrap())
            }))
        }
    }
}

fn decode_packed_numeric_bytes<T, F>(
    bytes: &[u8],
    stride: usize,
    declared_length: Option<usize>,
    from_le: F,
) -> Vec<T>
where
    F: Fn(&[u8]) -> T,
{
    let available = (bytes.len() - bytes.len() % stride) / stride;
    let target = declared_length.unwrap_or(available).min(available);
    if target == 0 {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(target);
    for chunk in bytes[..target * stride].chunks_exact(stride) {
        out.push(from_le(chunk));
    }
    out
}
