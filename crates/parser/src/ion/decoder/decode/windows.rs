use super::*;

fn find_windows(bounds: &[(f64, f64)], from: f64, to: f64) -> (usize, usize) {
    let start = bounds.partition_point(|&(_low, high)| high < from);
    let end = bounds.partition_point(|&(low, _high)| low <= to);
    (start, end.max(start))
}

#[derive(Debug, Clone, PartialEq)]
pub struct DataXY {
    pub x: NumericArray,
    pub y: NumericArray,
}

impl DataXY {
    pub(crate) fn empty() -> Self {
        Self {
            x: NumericArray::F64(Vec::new()),
            y: NumericArray::F64(Vec::new()),
        }
    }
}

impl NumericArray {
    pub fn to_f64(&self) -> Vec<f64> {
        match self {
            NumericArray::F64(values) => values.clone(),
            NumericArray::F32(values) => values.iter().map(|&value| value as f64).collect(),
            NumericArray::F16(values) => values.iter().copied().map(f16_bits_to_f64).collect(),
            NumericArray::I16(values) => values.iter().map(|&value| value as f64).collect(),
            NumericArray::I32(values) => values.iter().map(|&value| value as f64).collect(),
            NumericArray::I64(values) => values.iter().map(|&value| value as f64).collect(),
        }
    }

    pub(crate) fn extend_f64(&self, out: &mut Vec<f64>) {
        match self {
            NumericArray::F64(values) => out.extend_from_slice(values),
            NumericArray::F32(values) => out.extend(values.iter().map(|&value| value as f64)),
            NumericArray::F16(values) => out.extend(values.iter().copied().map(f16_bits_to_f64)),
            NumericArray::I16(values) => out.extend(values.iter().map(|&value| value as f64)),
            NumericArray::I32(values) => out.extend(values.iter().map(|&value| value as f64)),
            NumericArray::I64(values) => out.extend(values.iter().map(|&value| value as f64)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pixel {
    pub x: Range,
    pub y: Range,
    pub z: Range,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Select {
    All,
    Rt(Range),
    Area(Pixel),
}

pub struct Window<'a> {
    pub index: usize,
    pub summary: &'a ScanSummary,
    pub mz: &'a [f64],
    pub intensity: &'a [f64],
}

fn position_in_range(value: u32, range: Range) -> bool {
    let value = value as f64;
    value >= range.from && value <= range.to
}

fn scan_is_selected(select: &Select, summary: &ScanSummary) -> bool {
    match select {
        Select::All => true,
        Select::Rt(range) => summary.rt >= range.from && summary.rt <= range.to,
        Select::Area(pixel) => {
            position_in_range(summary.position_x, pixel.x)
                && position_in_range(summary.position_y, pixel.y)
                && position_in_range(summary.position_z, pixel.z)
        }
    }
}

fn scan_summary_from_record(record: &SpectrumSummary) -> ScanSummary {
    ScanSummary {
        rt: record.rt,
        rt_unit: TimeUnit::from_code(record.rt_unit),
        ms_level: record.ms_level,
        polarity: record.polarity,
        selected_ion_mz: record.selected_ion_mz,
        base_peak_mz: record.base_peak_mz,
        base_peak_int: record.base_peak_int,
        total_ion_current: record.total_ion_current,
        position_x: record.position_x,
        position_y: record.position_y,
        position_z: record.position_z,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Spec,
    Chrom,
}

#[derive(Debug, Clone, Copy)]
pub struct ItemSlice {
    pub item_index: u64,
    pub array_ref_index: u64,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct WindowRead {
    pub(crate) mz_ref: ArrayRef,
    pub(crate) intensity_ref: ArrayRef,
}

fn empty_array(dtype: u8) -> NumericArray {
    match dtype {
        FILE_DTYPE_F32 => NumericArray::F32(Vec::new()),
        FILE_DTYPE_F16 => NumericArray::F16(Vec::new()),
        FILE_DTYPE_I16 => NumericArray::I16(Vec::new()),
        FILE_DTYPE_I32 => NumericArray::I32(Vec::new()),
        FILE_DTYPE_I64 => NumericArray::I64(Vec::new()),
        _ => NumericArray::F64(Vec::new()),
    }
}

fn group_dtype(group: &ArrayGroup) -> u8 {
    group.refs.first().map(|array_ref| array_ref.dtype).unwrap_or(FILE_DTYPE_F64)
}

fn value_at(array: &NumericArray, index: usize) -> f64 {
    match array {
        NumericArray::F64(values) => values[index],
        NumericArray::F32(values) => values[index] as f64,
        NumericArray::F16(values) => f16_bits_to_f64(values[index]),
        NumericArray::I16(values) => values[index] as f64,
        NumericArray::I32(values) => values[index] as f64,
        NumericArray::I64(values) => values[index] as f64,
    }
}

fn append_range(dst: &mut NumericArray, src: &NumericArray, start: usize, end: usize) {
    match (dst, src) {
        (NumericArray::F64(d), NumericArray::F64(s)) => d.extend_from_slice(&s[start..end]),
        (NumericArray::F32(d), NumericArray::F32(s)) => d.extend_from_slice(&s[start..end]),
        (NumericArray::F16(d), NumericArray::F16(s)) => d.extend_from_slice(&s[start..end]),
        (NumericArray::I16(d), NumericArray::I16(s)) => d.extend_from_slice(&s[start..end]),
        (NumericArray::I32(d), NumericArray::I32(s)) => d.extend_from_slice(&s[start..end]),
        (NumericArray::I64(d), NumericArray::I64(s)) => d.extend_from_slice(&s[start..end]),
        _ => {}
    }
}

fn range_in_sorted(x: &NumericArray, low: f64, high: f64, paired: usize) -> (usize, usize) {
    match x {
        NumericArray::F64(values) => {
            let values = &values[..paired];
            (
                values.partition_point(|&value| value < low),
                values.partition_point(|&value| value <= high),
            )
        }
        NumericArray::F32(values) => {
            let values = &values[..paired];
            (
                values.partition_point(|&value| (value as f64) < low),
                values.partition_point(|&value| (value as f64) <= high),
            )
        }
        _ => (0, paired),
    }
}

fn is_sorted(x: &NumericArray, len: usize) -> bool {
    (1..len).all(|index| value_at(x, index - 1) <= value_at(x, index))
}

pub(crate) fn keep_pairs_sorted(
    x: &NumericArray,
    y: &NumericArray,
    low: f64,
    high: f64,
    x_out: &mut NumericArray,
    y_out: &mut NumericArray,
) {
    let paired = x.len().min(y.len());
    let (start, end) = range_in_sorted(x, low, high, paired);
    append_range(x_out, x, start, end);
    append_range(y_out, y, start, end);
}

pub(crate) fn keep_pairs(
    x: &NumericArray,
    y: &NumericArray,
    low: f64,
    high: f64,
    x_out: &mut NumericArray,
    y_out: &mut NumericArray,
) {
    let paired = x.len().min(y.len());
    if is_sorted(x, paired) {
        keep_pairs_sorted(x, y, low, high, x_out, y_out);
        return;
    }
    for index in 0..paired {
        let value = value_at(x, index);
        if value >= low && value <= high {
            append_range(x_out, x, index, index + 1);
            append_range(y_out, y, index, index + 1);
        }
    }
}

impl IonReader {
    pub fn read_spectrum_window(
        &mut self,
        index: usize,
        x_array_accession: u32,
        y_array_accession: u32,
        low: f64,
        high: f64,
    ) -> IonResult<DataXY> {
        self.read_spectrum_window_inner(index, x_array_accession, y_array_accession, low, high)
    }

    pub fn require_bounds(&mut self) -> IonResult<()> {
        self.ensure_spec_window_bounds()
    }

    pub(crate) fn get_spectrum_mz_windows(
        &mut self,
        scan_index: usize,
        mz_from: f64,
        mz_to: f64,
    ) -> IonResult<Vec<WindowRead>> {
        if !mz_from.is_finite() || !mz_to.is_finite() {
            return Err("mz window bounds must be finite".into());
        }
        if mz_from > mz_to {
            return Err("mz window: from is greater than to".into());
        }
        if scan_index >= self.header.spectrum_count as usize {
            return Err("spectrum index out of range".into());
        }

        self.require_bounds()?;

        let array_refs =
            read_array_refs_from_buffers(&self.spec_entries_buf, &self.spec_array_refs, scan_index)
                .ok_or_else(|| IonError::from("spectrum has no array refs"))?;
        let ref_start = array_ref_start_for_item(&self.spec_entries_buf, scan_index)
            .ok_or_else(|| IonError::from("spectrum has no array ref start"))?;

        let groups = group_arrays(array_refs.as_slice())?;

        let mut mz_group = None;
        let mut intensity_group = None;
        let mut position = 0u64;
        for group in &groups {
            if group.array_type == crate::accessions::MZ_ARRAY && mz_group.is_none() {
                mz_group = Some((position, group));
            } else if group.array_type == ACC_INT && intensity_group.is_none() {
                intensity_group = Some((position, group));
            }
            position += group.refs.len() as u64;
        }

        let (Some((mz_position, mz_group)), Some((_, intensity_group))) =
            (mz_group, intensity_group)
        else {
            return Err("spectrum is missing the mz or intensity array".into());
        };

        if mz_group.refs.len() != intensity_group.refs.len() {
            return Err("spectrum mz and intensity window counts differ".into());
        }

        let bounds = match &self.spec_window_bounds {
            WindowBoundsCache::Loaded(index) => index,
            _ => return Err(IonError::MissingSpectrumBounds),
        };

        let mz_ref_base = ref_start + mz_position;
        let window_count = mz_group.refs.len();
        let mut window_bounds = Vec::with_capacity(window_count);
        for segment_index in 0..window_count {
            window_bounds.push(bounds.require(mz_ref_base + segment_index as u64)?);
        }

        let (start, end) = find_windows(&window_bounds, mz_from, mz_to);
        let mut windows = Vec::with_capacity(end - start);
        for segment_index in start..end {
            windows.push(WindowRead {
                mz_ref: mz_group.refs[segment_index],
                intensity_ref: intensity_group.refs[segment_index],
            });
        }

        Ok(windows)
    }

    pub(crate) fn spec_block_byte_range(&self, block_id: u32) -> IonResult<ByteRange> {
        self.spec_container
            .block_byte_range(block_id)
            .ok_or_else(|| IonError::from("spectrum block id out of range"))
    }

    pub fn byte_ranges(&mut self, scan_index: usize, mz: Range) -> IonResult<Vec<ByteRange>> {
        let windows = self.get_spectrum_mz_windows(scan_index, mz.from, mz.to)?;

        let mut block_ids = Vec::with_capacity(windows.len() * 2);
        for window in &windows {
            block_ids.push(window.mz_ref.block_id);
            block_ids.push(window.intensity_ref.block_id);
        }
        block_ids.sort_unstable();
        block_ids.dedup();

        let mut ranges = Vec::with_capacity(block_ids.len());
        for block_id in block_ids {
            ranges.push(self.spec_block_byte_range(block_id)?);
        }
        ranges.sort_unstable_by_key(|range| (range.offset, range.length));
        Ok(ranges)
    }

    pub fn read_window(&mut self, index: usize, mz: Range) -> IonResult<DataXY> {
        let windows = self.get_spectrum_mz_windows(index, mz.from, mz.to)?;

        let mz_dtype = windows.first().map(|s| s.mz_ref.dtype).unwrap_or(FILE_DTYPE_F64);
        let intensity_dtype = windows
            .first()
            .map(|s| s.intensity_ref.dtype)
            .unwrap_or(FILE_DTYPE_F64);
        let mut mz_out = empty_array(mz_dtype);
        let mut intensity_out = empty_array(intensity_dtype);
        for window in windows {
            let mz_segment = self.read_array_typed(&window.mz_ref)?;
            let intensity_segment = self.read_array_typed(&window.intensity_ref)?;
            keep_pairs_sorted(
                &mz_segment,
                &intensity_segment,
                mz.from,
                mz.to,
                &mut mz_out,
                &mut intensity_out,
            );
        }

        Ok(DataXY { x: mz_out, y: intensity_out })
    }

    pub fn scans_in(
        &mut self,
        mz: Range,
        select: Select,
        ms_level: Option<u8>,
        visit: &mut dyn FnMut(&Window),
    ) -> IonResult<()> {
        self.require_bounds()?;
        let count = self.header.spectrum_count as usize;
        let mut mz_out: Vec<f64> = Vec::new();
        let mut intensity_out: Vec<f64> = Vec::new();
        for index in 0..count {
            let Some(record) = self.spec_summary(index) else {
                continue;
            };
            if let Some(level) = ms_level {
                if record.ms_level != level {
                    continue;
                }
            }
            let summary = scan_summary_from_record(&record);
            if !scan_is_selected(&select, &summary) {
                continue;
            }
            let Some(refs) = self.spectrum_arrays(index) else {
                continue;
            };
            let has_mz = refs.iter().any(|r| r.array_type == crate::accessions::MZ_ARRAY);
            let has_intensity = refs.iter().any(|r| r.array_type == ACC_INT);
            if !has_mz || !has_intensity {
                continue;
            }
            let data = self.read_window(index, mz)?;
            mz_out.clear();
            data.x.extend_f64(&mut mz_out);
            intensity_out.clear();
            data.y.extend_f64(&mut intensity_out);
            let window = Window {
                index,
                summary: &summary,
                mz: &mz_out,
                intensity: &intensity_out,
            };
            visit(&window);
        }
        Ok(())
    }

    pub(crate) fn read_spectrum_window_inner(
        &mut self,
        index: usize,
        x_array_accession: u32,
        y_array_accession: u32,
        low: f64,
        high: f64,
    ) -> IonResult<DataXY> {
        if index >= self.header.spectrum_count as usize {
            return Err("spectrum index out of range".into());
        }

        if low > high {
            return Ok(DataXY::empty());
        }

        let Some(array_refs) =
            read_array_refs_from_buffers(&self.spec_entries_buf, &self.spec_array_refs, index)
        else {
            return Ok(DataXY::empty());
        };
        let Some(ref_start) = array_ref_start_for_item(&self.spec_entries_buf, index) else {
            return Ok(DataXY::empty());
        };

        let groups = group_arrays(array_refs.as_slice())?;

        let mut x_group = None;
        let mut y_group = None;
        let mut position = 0u64;
        for group in &groups {
            if group.array_type == x_array_accession && x_group.is_none() {
                x_group = Some((position, group));
            } else if group.array_type == y_array_accession && y_group.is_none() {
                y_group = Some((position, group));
            }
            position += group.refs.len() as u64;
        }

        let (Some((x_position, x_group)), Some((_, y_group))) = (x_group, y_group) else {
            return Ok(DataXY::empty());
        };

        if x_group.refs.len() == y_group.refs.len() {
            let _ = self.ensure_spec_window_bounds();
            if let Some(window) =
                self.try_fast_window(ref_start + x_position, x_group, y_group, low, high)?
            {
                return Ok(window);
            }
        }

        self.read_full_window(x_group, y_group, low, high)
    }

    pub(crate) fn try_fast_window(
        &mut self,
        x_ref_base: u64,
        x_group: &ArrayGroup,
        y_group: &ArrayGroup,
        low: f64,
        high: f64,
    ) -> IonResult<Option<DataXY>> {
        let kept_segments = {
            let WindowBoundsCache::Loaded(bounds) = &self.spec_window_bounds else {
                return Ok(None);
            };
            let mut kept = Vec::with_capacity(x_group.refs.len());
            for segment_index in 0..x_group.refs.len() {
                let global_ref_index = x_ref_base + segment_index as u64;
                let Some((window_low, window_high)) = bounds.get(global_ref_index) else {
                    return Ok(None);
                };
                let overlaps_window = window_low <= high && window_high >= low;
                if overlaps_window {
                    kept.push(segment_index);
                }
            }
            kept
        };

        let mut x_out = empty_array(group_dtype(x_group));
        let mut y_out = empty_array(group_dtype(y_group));
        for segment_index in kept_segments {
            let x_segment = self.read_array_typed(&x_group.refs[segment_index])?;
            let y_segment = self.read_array_typed(&y_group.refs[segment_index])?;
            keep_pairs_sorted(&x_segment, &y_segment, low, high, &mut x_out, &mut y_out);
        }

        Ok(Some(DataXY { x: x_out, y: y_out }))
    }

    pub(crate) fn read_full_window(
        &mut self,
        x_group: &ArrayGroup,
        y_group: &ArrayGroup,
        low: f64,
        high: f64,
    ) -> IonResult<DataXY> {
        let x = self.read_group_typed(x_group)?;
        let y = self.read_group_typed(y_group)?;

        let mut x_out = empty_array(group_dtype(x_group));
        let mut y_out = empty_array(group_dtype(y_group));
        keep_pairs(&x, &y, low, high, &mut x_out, &mut y_out);
        Ok(DataXY { x: x_out, y: y_out })
    }

    fn read_group_typed(&mut self, group: &ArrayGroup) -> IonResult<NumericArray> {
        let mut out = empty_array(group_dtype(group));
        for array_ref in &group.refs {
            let window = self.read_array_typed(array_ref)?;
            append_range(&mut out, &window, 0, window.len());
        }
        Ok(out)
    }

    pub fn candidate_items(
        &mut self,
        target: Target,
        axis_accession: u32,
        lo: f64,
        hi: f64,
    ) -> IonResult<Vec<ItemSlice>> {
        use crate::ion::axes::axis_of;

        if axis_of(axis_accession).is_none() {
            return Ok(Vec::new());
        }

        let (entries_buf, array_refs_buf, window_bounds) = match target {
            Target::Spec => {
                let _ = self.ensure_spec_window_bounds();
                (
                    &self.spec_entries_buf,
                    &self.spec_array_refs,
                    &self.spec_window_bounds,
                )
            }
            Target::Chrom => {
                self.ensure_chrom_window_bounds();
                (
                    &self.chrom_entries_buf,
                    &self.chrom_array_refs,
                    &self.chrom_window_bounds,
                )
            }
        };

        let item_count = match target {
            Target::Spec => self.header.spectrum_count,
            Target::Chrom => self.header.chrom_count,
        };

        let bounds = match window_bounds {
            WindowBoundsCache::Loaded(b) => Some(b),
            _ => None,
        };

        let mut result = Vec::new();

        for item_idx in 0..item_count {
            let entry_offset = (item_idx as usize) * INDEX_ENTRY_BYTES;
            if entry_offset + INDEX_ENTRY_BYTES > entries_buf.len() {
                break;
            }
            let entry = &entries_buf[entry_offset..entry_offset + INDEX_ENTRY_BYTES];
            let first_aref = u64::from_le_bytes(entry[0..8].try_into().unwrap());
            let aref_count = u64::from_le_bytes(entry[8..16].try_into().unwrap());

            for segment_index in 0..aref_count {
                let array_ref_index = first_aref + segment_index;

                let aref_offset = (array_ref_index as usize) * ARRAY_REF_BYTES;
                if aref_offset + ARRAY_REF_BYTES > array_refs_buf.len() {
                    continue;
                }
                let aref_bytes = &array_refs_buf[aref_offset..aref_offset + ARRAY_REF_BYTES];
                let array_type = u32::from_le_bytes(aref_bytes[20..24].try_into().unwrap());

                if array_type != axis_accession {
                    continue;
                }

                let include = match bounds.as_ref() {
                    Some(b) => {
                        if let Some((window_low, window_high)) = b.get(array_ref_index) {
                            window_low <= hi && window_high >= lo
                        } else {
                            true
                        }
                    }
                    None => true,
                };

                if include {
                    result.push(ItemSlice {
                        item_index: item_idx,
                        array_ref_index,
                    });
                }
            }
        }

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::find_windows;

    #[test]
    fn find_windows_returns_the_overlapping_run() {
        let bounds = [
            (100.0, 150.0),
            (150.0, 200.0),
            (200.0, 250.0),
            (250.0, 300.0),
        ];
        assert_eq!(find_windows(&bounds, 160.0, 240.0), (1, 3));
        assert_eq!(find_windows(&bounds, 0.0, 1000.0), (0, 4));
        assert_eq!(find_windows(&bounds, 100.0, 100.0), (0, 1));
        assert_eq!(find_windows(&bounds, 400.0, 500.0), (4, 4));
        assert_eq!(find_windows(&bounds, 0.0, 50.0), (0, 0));
    }
}
