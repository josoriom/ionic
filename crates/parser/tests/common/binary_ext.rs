use ionic::mzml::structs::NumericArray;

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

impl BinaryDataExt for NumericArray {
    fn len(&self) -> usize {
        match self {
            NumericArray::F64(v) => v.len(),
            NumericArray::F32(v) => v.len(),
            NumericArray::F16(v) => v.len(),
            NumericArray::I64(v) => v.len(),
            NumericArray::I32(v) => v.len(),
            NumericArray::I16(v) => v.len(),
        }
    }

    fn to_f64_vec(&self) -> Vec<f64> {
        match self {
            NumericArray::F64(v) => v.clone(),
            NumericArray::F32(v) => v.iter().map(|x| *x as f64).collect(),
            NumericArray::F16(v) => v.iter().map(|x| *x as f64).collect(),
            NumericArray::I64(v) => v.iter().map(|x| *x as f64).collect(),
            NumericArray::I32(v) => v.iter().map(|x| *x as f64).collect(),
            NumericArray::I16(v) => v.iter().map(|x| *x as f64).collect(),
        }
    }

    fn first_f64(&self) -> Option<f64> {
        match self {
            NumericArray::F64(v) => v.first().copied(),
            NumericArray::F32(v) => v.first().map(|x| *x as f64),
            NumericArray::F16(v) => v.first().map(|x| *x as f64),
            NumericArray::I64(v) => v.first().map(|x| *x as f64),
            NumericArray::I32(v) => v.first().map(|x| *x as f64),
            NumericArray::I16(v) => v.first().map(|x| *x as f64),
        }
    }

    fn to_le_bytes(&self) -> Vec<u8> {
        match self {
            NumericArray::F64(v) => v.iter().flat_map(|x| x.to_le_bytes()).collect(),
            NumericArray::F32(v) => v.iter().flat_map(|x| x.to_le_bytes()).collect(),
            NumericArray::F16(v) => v.iter().flat_map(|x| x.to_le_bytes()).collect(),
            NumericArray::I64(v) => v.iter().flat_map(|x| x.to_le_bytes()).collect(),
            NumericArray::I32(v) => v.iter().flat_map(|x| x.to_le_bytes()).collect(),
            NumericArray::I16(v) => v.iter().flat_map(|x| x.to_le_bytes()).collect(),
        }
    }

    fn variant_name(&self) -> &'static str {
        match self {
            NumericArray::F64(_) => "f64",
            NumericArray::F32(_) => "f32",
            NumericArray::F16(_) => "f16",
            NumericArray::I64(_) => "i64",
            NumericArray::I32(_) => "i32",
            NumericArray::I16(_) => "i16",
        }
    }
}
