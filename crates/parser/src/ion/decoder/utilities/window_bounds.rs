use crate::ion::{IonError, IonResult};

const SEGMENT_BOUND_SIZE: usize = 24;

pub(crate) struct WindowBound {
    pub(crate) array_ref_index: u64,
    pub(crate) low: f64,
    pub(crate) high: f64,
}

pub(crate) struct WindowBoundsIndex {
    rows: Vec<WindowBound>,
}

impl WindowBoundsIndex {
    pub(crate) fn get(&self, array_ref_index: u64) -> Option<(f64, f64)> {
        self.rows
            .binary_search_by_key(&array_ref_index, |row| row.array_ref_index)
            .ok()
            .map(|index| {
                let row = &self.rows[index];
                (row.low, row.high)
            })
    }

    pub(crate) fn require(&self, array_ref_index: u64) -> IonResult<(f64, f64)> {
        self.get(array_ref_index).ok_or_else(|| {
            IonError::MalformedSpectrumBounds(format!(
                "no window bounds row for array ref {array_ref_index}"
            ))
        })
    }

    pub(crate) fn from_bytes(bytes: &[u8], spec_array_ref_count: u64) -> IonResult<Self> {
        if !bytes.len().is_multiple_of(SEGMENT_BOUND_SIZE) {
            return Err("window bounds: plain_length not divisible by 24".into());
        }

        let row_count = bytes.len() / SEGMENT_BOUND_SIZE;
        let mut rows: Vec<WindowBound> = Vec::with_capacity(row_count);

        for i in 0..row_count {
            let offset = i * SEGMENT_BOUND_SIZE;
            let array_ref_index = u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap());
            let low = f64::from_le_bytes(bytes[offset + 8..offset + 16].try_into().unwrap());
            let high = f64::from_le_bytes(bytes[offset + 16..offset + 24].try_into().unwrap());

            if i > 0 && rows[i - 1].array_ref_index >= array_ref_index {
                return Err("window bounds: keys not strictly ascending".into());
            }
            if array_ref_index >= spec_array_ref_count {
                return Err("window bounds: array_ref_index out of range".into());
            }
            if !low.is_finite() || !high.is_finite() || low > high {
                return Err("window bounds: invalid low/high value".into());
            }

            rows.push(WindowBound {
                array_ref_index,
                low,
                high,
            });
        }

        Ok(WindowBoundsIndex { rows })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row_bytes(array_ref_index: u64, low: f64, high: f64) -> Vec<u8> {
        let mut buf = Vec::with_capacity(SEGMENT_BOUND_SIZE);
        buf.extend_from_slice(&array_ref_index.to_le_bytes());
        buf.extend_from_slice(&low.to_le_bytes());
        buf.extend_from_slice(&high.to_le_bytes());
        buf
    }

    #[test]
    fn from_bytes_parses_sorted_rows() {
        let mut bytes = row_bytes(0, 100.0, 200.0);
        bytes.extend(row_bytes(3, 200.0, 300.0));
        let index = WindowBoundsIndex::from_bytes(&bytes, 4).unwrap();
        assert_eq!(index.get(0), Some((100.0, 200.0)));
        assert_eq!(index.get(3), Some((200.0, 300.0)));
        assert_eq!(index.get(1), None);
    }

    #[test]
    fn from_bytes_rejects_unaligned_length() {
        let bytes = vec![0u8; 23];
        assert!(WindowBoundsIndex::from_bytes(&bytes, 4).is_err());
    }

    #[test]
    fn from_bytes_rejects_non_ascending_keys() {
        let mut bytes = row_bytes(2, 1.0, 2.0);
        bytes.extend(row_bytes(2, 3.0, 4.0));
        assert!(WindowBoundsIndex::from_bytes(&bytes, 8).is_err());
    }

    #[test]
    fn from_bytes_rejects_index_out_of_range() {
        let bytes = row_bytes(5, 1.0, 2.0);
        assert!(WindowBoundsIndex::from_bytes(&bytes, 5).is_err());
    }

    #[test]
    fn from_bytes_rejects_low_above_high() {
        let bytes = row_bytes(0, 9.0, 1.0);
        assert!(WindowBoundsIndex::from_bytes(&bytes, 4).is_err());
    }

    #[test]
    fn from_bytes_rejects_non_finite_values() {
        let nan = row_bytes(0, f64::NAN, 1.0);
        assert!(WindowBoundsIndex::from_bytes(&nan, 4).is_err());
        let inf = row_bytes(0, 1.0, f64::INFINITY);
        assert!(WindowBoundsIndex::from_bytes(&inf, 4).is_err());
    }

    #[test]
    fn get_returns_none_on_empty() {
        let index = WindowBoundsIndex::from_bytes(&[], 0).unwrap();
        assert_eq!(index.get(0), None);
    }
}
