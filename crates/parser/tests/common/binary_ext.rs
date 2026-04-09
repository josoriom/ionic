use ionic::mzml::structs::BinaryData;

pub(crate) trait BinaryDataExt {
    fn len(&self) -> usize;

    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn to_f64_vec(&self) -> Vec<f64>;

    fn first_f64(&self) -> Option<f64>;

    fn to_le_bytes(&self) -> Vec<u8>;

    fn variant_name(&self) -> &'static str;
}

impl BinaryDataExt for BinaryData {
    fn len(&self) -> usize {
        match self {
            BinaryData::F64(v) => v.len(),
            BinaryData::F32(v) => v.len(),
            BinaryData::F16(v) => v.len(),
            BinaryData::I64(v) => v.len(),
            BinaryData::I32(v) => v.len(),
            BinaryData::I16(v) => v.len(),
        }
    }

    fn to_f64_vec(&self) -> Vec<f64> {
        match self {
            BinaryData::F64(v) => v.clone(),
            BinaryData::F32(v) => v.iter().map(|x| *x as f64).collect(),
            BinaryData::F16(v) => v.iter().map(|x| *x as f64).collect(),
            BinaryData::I64(v) => v.iter().map(|x| *x as f64).collect(),
            BinaryData::I32(v) => v.iter().map(|x| *x as f64).collect(),
            BinaryData::I16(v) => v.iter().map(|x| *x as f64).collect(),
        }
    }

    fn first_f64(&self) -> Option<f64> {
        match self {
            BinaryData::F64(v) => v.first().copied(),
            BinaryData::F32(v) => v.first().map(|x| *x as f64),
            BinaryData::F16(v) => v.first().map(|x| *x as f64),
            BinaryData::I64(v) => v.first().map(|x| *x as f64),
            BinaryData::I32(v) => v.first().map(|x| *x as f64),
            BinaryData::I16(v) => v.first().map(|x| *x as f64),
        }
    }

    fn to_le_bytes(&self) -> Vec<u8> {
        match self {
            BinaryData::F64(v) => v.iter().flat_map(|x| x.to_le_bytes()).collect(),
            BinaryData::F32(v) => v.iter().flat_map(|x| x.to_le_bytes()).collect(),
            BinaryData::F16(v) => v.iter().flat_map(|x| x.to_le_bytes()).collect(),
            BinaryData::I64(v) => v.iter().flat_map(|x| x.to_le_bytes()).collect(),
            BinaryData::I32(v) => v.iter().flat_map(|x| x.to_le_bytes()).collect(),
            BinaryData::I16(v) => v.iter().flat_map(|x| x.to_le_bytes()).collect(),
        }
    }

    fn variant_name(&self) -> &'static str {
        match self {
            BinaryData::F64(_) => "f64",
            BinaryData::F32(_) => "f32",
            BinaryData::F16(_) => "f16",
            BinaryData::I64(_) => "i64",
            BinaryData::I32(_) => "i32",
            BinaryData::I16(_) => "i16",
        }
    }
}
