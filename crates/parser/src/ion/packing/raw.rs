use super::{Dtype, IonResult, Packing, PackingId, PackingInput};

pub(crate) static RAW: Raw = Raw;
pub(crate) struct Raw;

impl Packing for Raw {
    fn id(&self) -> PackingId {
        PackingId::Raw
    }

    fn is_generic(&self) -> bool {
        true
    }

    fn supports(&self, _dtype: Dtype) -> bool {
        true
    }

    fn encode(&self, input: PackingInput<'_>, out: &mut Vec<u8>) -> IonResult<()> {
        if let PackingInput::Bytes(b) = input {
            out.extend_from_slice(b);
        }
        Ok(())
    }

    fn decode(&self, input: &[u8], _dtype: Dtype, out: &mut Vec<u8>) -> IonResult<()> {
        out.extend_from_slice(input);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_copies_bytes() {
        let mut out = Vec::new();
        RAW.encode(PackingInput::Bytes(&[1, 2, 3, 4]), &mut out)
            .unwrap();
        assert_eq!(out, [1, 2, 3, 4]);
    }

    #[test]
    fn encode_non_bytes_input_is_noop() {
        let mut out = Vec::new();
        RAW.encode(PackingInput::F64(&[1.0, 2.0]), &mut out)
            .unwrap();
        assert!(out.is_empty());
    }

    #[test]
    fn decode_copies_bytes() {
        let mut out = Vec::new();
        RAW.decode(&[5, 6, 7, 8], Dtype::F64, &mut out).unwrap();
        assert_eq!(out, [5, 6, 7, 8]);
    }

    #[test]
    fn decode_empty_input_produces_no_output() {
        let mut out = Vec::new();
        RAW.decode(&[], Dtype::F64, &mut out).unwrap();
        assert!(out.is_empty());
    }

    #[test]
    fn bytes_roundtrip() {
        let input = b"hello world 12345678";
        let mut enc = Vec::new();
        RAW.encode(PackingInput::Bytes(input), &mut enc).unwrap();
        let mut dec = Vec::new();
        RAW.decode(&enc, Dtype::F64, &mut dec).unwrap();
        assert_eq!(dec.as_slice(), input.as_ref());
    }

    #[test]
    fn is_generic() {
        assert!(RAW.is_generic());
    }
}
