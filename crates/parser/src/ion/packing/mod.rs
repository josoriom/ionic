use crate::ion::{IonError, IonResult};

pub(crate) mod byte_shuffle;
pub(crate) mod delta_shuffle;
pub(crate) mod raw;

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Dtype {
    F64 = 1,
    F32 = 2,
    F16 = 3,
    I16 = 4,
    I32 = 5,
    I64 = 6,
}

impl Dtype {
    pub(crate) fn from_byte(b: u8) -> IonResult<Self> {
        match b {
            1 => Ok(Self::F64),
            2 => Ok(Self::F32),
            3 => Ok(Self::F16),
            4 => Ok(Self::I16),
            5 => Ok(Self::I32),
            6 => Ok(Self::I64),
            _ => Err(IonError::BadDtype {
                dtype: b,
                kind: "packing dtype",
            }),
        }
    }

    pub(crate) fn byte_stride(self) -> usize {
        match self {
            Self::F64 | Self::I64 => 8,
            Self::F32 | Self::I32 => 4,
            Self::F16 | Self::I16 => 2,
        }
    }
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PackingId {
    Raw = 0,
    ByteShuffle = 1,
    DeltaShuffle = 2,
    DeltaSquaredVByte = 3,
    Alp = 4,
    Chimp = 5,
}

impl PackingId {
    pub(crate) fn from_byte(b: u8) -> IonResult<Self> {
        match b {
            0 => Ok(Self::Raw),
            1 => Ok(Self::ByteShuffle),
            2 => Ok(Self::DeltaShuffle),
            3 => Ok(Self::DeltaSquaredVByte),
            4 => Ok(Self::Alp),
            5 => Ok(Self::Chimp),
            _ => Err(IonError::UnsupportedPacking(b)),
        }
    }
}

pub(crate) enum PackingInput<'a> {
    F32(&'a [f32]),
    F64(&'a [f64]),
    I16(&'a [i16]),
    I32(&'a [i32]),
    I64(&'a [i64]),
    Bytes(&'a [u8]),
}

// This trait centralizes all data transformation strategies.
pub(crate) trait Packing: Send + Sync {
    fn id(&self) -> PackingId;

    fn min_input_len(&self) -> usize {
        1
    }
    /// Generic packings are universally applicable. False if specialized for certain dtypes or requires pairing with other transforms.
    fn is_generic(&self) -> bool {
        false
    }

    /// Returns true if this packing supports the given dtype.
    fn supports(&self, dtype: Dtype) -> bool;

    fn encode(&self, input: PackingInput<'_>, out: &mut Vec<u8>) -> IonResult<()>;

    fn decode(&self, input: &[u8], dtype: Dtype, out: &mut Vec<u8>) -> IonResult<()>;
}

pub(crate) fn packing_for(
    array_type: u32,
    dtype: Dtype,
    element_count: usize,
) -> &'static dyn Packing {
    use crate::accessions::MZ_ARRAY;
    use delta_shuffle::DELTA_SHUFFLE;
    use raw::RAW;

    let candidate: &'static dyn Packing = match (array_type, dtype) {
        (MZ_ARRAY, Dtype::F64) => &DELTA_SHUFFLE,
        _ => &RAW,
    };

    if element_count < candidate.min_input_len() {
        return &RAW;
    }

    candidate
}

pub(crate) fn packing_by_id(id: PackingId) -> &'static dyn Packing {
    use byte_shuffle::BYTE_SHUFFLE;
    use delta_shuffle::DELTA_SHUFFLE;
    use raw::RAW;

    match id {
        PackingId::Raw => &RAW,
        PackingId::ByteShuffle => &BYTE_SHUFFLE,
        PackingId::DeltaShuffle => &DELTA_SHUFFLE,
        PackingId::DeltaSquaredVByte => &RAW,
        PackingId::Alp => &RAW,
        PackingId::Chimp => &RAW,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accessions::MZ_ARRAY;

    #[test]
    fn packing_id_from_byte_roundtrip() {
        for b in 0u8..=5 {
            let id = PackingId::from_byte(b).unwrap();
            assert_eq!(id as u8, b);
        }
        assert!(PackingId::from_byte(6).is_err());
    }

    #[test]
    fn dtype_from_byte_roundtrip() {
        for b in 1u8..=6 {
            let d = Dtype::from_byte(b).unwrap();
            assert_eq!(d as u8, b);
        }
        assert!(Dtype::from_byte(0).is_err());
        assert!(Dtype::from_byte(7).is_err());
    }

    #[test]
    fn dtype_byte_stride_correct() {
        assert_eq!(Dtype::F64.byte_stride(), 8);
        assert_eq!(Dtype::I64.byte_stride(), 8);
        assert_eq!(Dtype::F32.byte_stride(), 4);
        assert_eq!(Dtype::I32.byte_stride(), 4);
        assert_eq!(Dtype::F16.byte_stride(), 2);
        assert_eq!(Dtype::I16.byte_stride(), 2);
    }

    #[test]
    fn packing_for_mz_f64_returns_delta_shuffle() {
        let p = packing_for(MZ_ARRAY, Dtype::F64, 100);
        assert_eq!(p.id(), PackingId::DeltaShuffle);
    }

    #[test]
    fn packing_for_non_mz_returns_raw() {
        let p = packing_for(0, Dtype::F64, 100);
        assert_eq!(p.id(), PackingId::Raw);
    }

    #[test]
    fn packing_for_mz_non_f64_returns_raw() {
        let p = packing_for(MZ_ARRAY, Dtype::F32, 100);
        assert_eq!(p.id(), PackingId::Raw);
    }

    #[test]
    fn packing_by_id_covers_all_variants() {
        for b in 0u8..=5 {
            let id = PackingId::from_byte(b).unwrap();
            let _ = packing_by_id(id);
        }
    }
}
