use crate::ion::IonResult;

pub(crate) struct WindowEntry {
    pub(crate) spectrum_index: u32,
    pub(crate) mz_address: u32,
    pub(crate) intensity_address: u32,
}

pub(crate) struct WindowDirectory {
    starts: Vec<u32>,
    spectrum_index: Vec<u32>,
    mz_address: Vec<u32>,
    intensity_address: Vec<u32>,
}

impl WindowDirectory {
    pub(crate) fn window_count(&self) -> usize {
        self.starts.len().saturating_sub(1)
    }

    pub(crate) fn window_range(&self, window: usize) -> std::ops::Range<usize> {
        self.starts[window] as usize..self.starts[window + 1] as usize
    }

    pub(crate) fn row(&self, position: usize) -> WindowEntry {
        WindowEntry {
            spectrum_index: self.spectrum_index[position],
            mz_address: self.mz_address[position],
            intensity_address: self.intensity_address[position],
        }
    }

    pub(crate) fn find_in_window(&self, window: usize, spectrum_index: u32) -> Option<WindowEntry> {
        let range = self.window_range(window);
        let found = self.spectrum_index[range.clone()]
            .binary_search(&spectrum_index)
            .ok()?;
        Some(self.row(range.start + found))
    }

    pub(crate) fn from_bytes(
        bytes: &[u8],
        array_address_count: u64,
        item_count: u64,
    ) -> IonResult<Self> {
        if bytes.len() < 8 {
            return Err("window directory: header too short".into());
        }
        let window_count = read_u32(bytes, 0) as usize;
        let entry_count = read_u32(bytes, 4) as usize;
        if window_count == 0 {
            return Err("window directory: window_count is zero".into());
        }

        let starts_len = window_count + 1;
        let expected_len = 8 + starts_len * 4 + entry_count * 4 * 3;
        if bytes.len() != expected_len {
            return Err("window directory: length does not match window_count and entry_count".into());
        }

        let mut starts = Vec::with_capacity(starts_len);
        for slot in 0..starts_len {
            starts.push(read_u32(bytes, 8 + slot * 4));
        }
        if starts[0] != 0 || starts[window_count] as usize != entry_count {
            return Err("window directory: starts must run from 0 to entry_count".into());
        }
        for pair in starts.windows(2) {
            if pair[1] < pair[0] {
                return Err("window directory: starts must be non-decreasing".into());
            }
        }

        let mut cursor = 8 + starts_len * 4;
        let spectrum_index = read_column(bytes, &mut cursor, entry_count);
        let mz_address = read_column(bytes, &mut cursor, entry_count);
        let intensity_address = read_column(bytes, &mut cursor, entry_count);

        for &spectrum in &spectrum_index {
            if spectrum as u64 >= item_count {
                return Err("window directory: spectrum_index out of range".into());
            }
        }
        for &address in mz_address.iter().chain(intensity_address.iter()) {
            if address as u64 >= array_address_count {
                return Err("window directory: array address index out of range".into());
            }
        }
        for window in 0..window_count {
            let range = starts[window] as usize..starts[window + 1] as usize;
            for pair in spectrum_index[range].windows(2) {
                if pair[1] <= pair[0] {
                    return Err("window directory: spectrum_index must increase within a window".into());
                }
            }
        }

        Ok(Self {
            starts,
            spectrum_index,
            mz_address,
            intensity_address,
        })
    }
}

fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

fn read_column(bytes: &[u8], cursor: &mut usize, count: usize) -> Vec<u32> {
    let mut column = Vec::with_capacity(count);
    for _ in 0..count {
        column.push(read_u32(bytes, *cursor));
        *cursor += 4;
    }
    column
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build(windows: &[&[(u32, u32, u32)]]) -> Vec<u8> {
        let entry_count: usize = windows.iter().map(|window| window.len()).sum();
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&(windows.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&(entry_count as u32).to_le_bytes());
        let mut start = 0u32;
        for window in windows {
            bytes.extend_from_slice(&start.to_le_bytes());
            start += window.len() as u32;
        }
        bytes.extend_from_slice(&start.to_le_bytes());
        for window in windows {
            for row in *window {
                bytes.extend_from_slice(&row.0.to_le_bytes());
            }
        }
        for window in windows {
            for row in *window {
                bytes.extend_from_slice(&row.1.to_le_bytes());
            }
        }
        for window in windows {
            for row in *window {
                bytes.extend_from_slice(&row.2.to_le_bytes());
            }
        }
        bytes
    }

    #[test]
    fn parses_and_finds_rows() {
        let bytes = build(&[&[(0, 0, 4), (1, 8, 12)], &[(0, 1, 5), (1, 9, 13)]]);
        let index = WindowDirectory::from_bytes(&bytes, 16, 2).unwrap();
        assert_eq!(index.window_count(), 2);
        assert_eq!(index.window_range(1), 2..4);
        let row = index.find_in_window(1, 1).unwrap();
        assert_eq!((row.spectrum_index, row.mz_address, row.intensity_address), (1, 9, 13));
        assert!(index.find_in_window(1, 5).is_none());
    }

    #[test]
    fn rejects_length_mismatch() {
        let mut bytes = build(&[&[(0, 0, 4)]]);
        bytes.push(0);
        assert!(WindowDirectory::from_bytes(&bytes, 16, 2).is_err());
    }

    #[test]
    fn rejects_ref_index_out_of_range() {
        let bytes = build(&[&[(0, 99, 4)]]);
        assert!(WindowDirectory::from_bytes(&bytes, 16, 2).is_err());
    }

    #[test]
    fn rejects_spectrum_index_out_of_range() {
        let bytes = build(&[&[(9, 0, 4)]]);
        assert!(WindowDirectory::from_bytes(&bytes, 16, 2).is_err());
    }

    #[test]
    fn rejects_unsorted_spectrum_within_window() {
        let bytes = build(&[&[(1, 0, 4), (0, 8, 12)]]);
        assert!(WindowDirectory::from_bytes(&bytes, 16, 2).is_err());
    }
}
