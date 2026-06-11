use super::*;

#[derive(Debug, Clone, PartialEq)]
pub struct ArrayWindow {
    pub x: Vec<f64>,
    pub y: Vec<f64>,
}

impl ArrayWindow {
    pub(crate) fn empty() -> Self {
        Self {
            x: Vec::new(),
            y: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct MzPeaks {
    pub mz: Vec<f64>,
    pub intensity: Vec<f64>,
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
pub(crate) struct SegmentRead {
    pub(crate) mz_ref: ArrayRef,
    pub(crate) intensity_ref: ArrayRef,
}

fn slice_is_non_decreasing(values: &[f64]) -> bool {
    values.windows(2).all(|pair| pair[0] <= pair[1])
}

pub(crate) fn keep_pairs_in_range_sorted(
    x: &[f64],
    y: &[f64],
    low: f64,
    high: f64,
    x_out: &mut Vec<f64>,
    y_out: &mut Vec<f64>,
) {
    let paired = x.len().min(y.len());
    let x = &x[..paired];
    let y = &y[..paired];
    let start = x.partition_point(|&value| value < low);
    let end = x.partition_point(|&value| value <= high);
    x_out.extend_from_slice(&x[start..end]);
    y_out.extend_from_slice(&y[start..end]);
}

pub(crate) fn keep_pairs_in_range(
    x: &[f64],
    y: &[f64],
    low: f64,
    high: f64,
    x_out: &mut Vec<f64>,
    y_out: &mut Vec<f64>,
) {
    let paired = x.len().min(y.len());
    let x = &x[..paired];
    let y = &y[..paired];

    if slice_is_non_decreasing(x) {
        keep_pairs_in_range_sorted(x, y, low, high, x_out, y_out);
        return;
    }
    for (position, &value) in x.iter().enumerate() {
        if value >= low && value <= high {
            x_out.push(value);
            y_out.push(y[position]);
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
    ) -> IonResult<ArrayWindow> {
        self.read_spectrum_window_inner(index, x_array_accession, y_array_accession, low, high)
    }

    pub fn require_bounds(&mut self) -> IonResult<()> {
        self.ensure_spec_segment_bounds()
    }

    pub(crate) fn get_spectrum_mz_segments(
        &mut self,
        scan_index: usize,
        mz_from: f64,
        mz_to: f64,
    ) -> IonResult<Vec<SegmentRead>> {
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
            return Err("spectrum mz and intensity segment counts differ".into());
        }

        let bounds = match &self.spec_segment_bounds {
            SegmentBoundsCache::Loaded(index) => index,
            _ => return Err(IonError::MissingSpectrumBounds),
        };

        let mz_ref_base = ref_start + mz_position;
        let mut segments = Vec::new();
        for segment_index in 0..mz_group.refs.len() {
            let row_index = mz_ref_base + segment_index as u64;
            let (segment_low, segment_high) = bounds.require(row_index)?;
            let overlaps = segment_low <= mz_to && segment_high >= mz_from;
            if overlaps {
                segments.push(SegmentRead {
                    mz_ref: mz_group.refs[segment_index],
                    intensity_ref: intensity_group.refs[segment_index],
                });
            }
        }

        Ok(segments)
    }

    pub(crate) fn spec_block_byte_range(&self, block_id: u32) -> IonResult<Range> {
        self.spec_container
            .block_byte_range(block_id)
            .ok_or_else(|| IonError::from("spectrum block id out of range"))
    }

    pub fn plan_mz_range(
        &mut self,
        scan_index: usize,
        mz_from: f64,
        mz_to: f64,
    ) -> IonResult<Vec<Range>> {
        let segments = self.get_spectrum_mz_segments(scan_index, mz_from, mz_to)?;

        let mut block_ids = Vec::with_capacity(segments.len() * 2);
        for segment in &segments {
            block_ids.push(segment.mz_ref.block_id);
            block_ids.push(segment.intensity_ref.block_id);
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

    pub fn read_mz_range(
        &mut self,
        scan_index: usize,
        mz_from: f64,
        mz_to: f64,
    ) -> IonResult<MzPeaks> {
        let segments = self.get_spectrum_mz_segments(scan_index, mz_from, mz_to)?;

        let mut mz = Vec::new();
        let mut intensity = Vec::new();
        let mut mz_segment = Vec::new();
        let mut intensity_segment = Vec::new();
        for segment in segments {
            self.read_array(&segment.mz_ref, &mut mz_segment)?;
            self.read_array(&segment.intensity_ref, &mut intensity_segment)?;
            keep_pairs_in_range_sorted(
                &mz_segment,
                &intensity_segment,
                mz_from,
                mz_to,
                &mut mz,
                &mut intensity,
            );
        }

        Ok(MzPeaks { mz, intensity })
    }

    pub(crate) fn read_spectrum_window_inner(
        &mut self,
        index: usize,
        x_array_accession: u32,
        y_array_accession: u32,
        low: f64,
        high: f64,
    ) -> IonResult<ArrayWindow> {
        if index >= self.header.spectrum_count as usize {
            return Err("spectrum index out of range".into());
        }

        if low > high {
            return Ok(ArrayWindow::empty());
        }

        let Some(array_refs) =
            read_array_refs_from_buffers(&self.spec_entries_buf, &self.spec_array_refs, index)
        else {
            return Ok(ArrayWindow::empty());
        };
        let Some(ref_start) = array_ref_start_for_item(&self.spec_entries_buf, index) else {
            return Ok(ArrayWindow::empty());
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
            return Ok(ArrayWindow::empty());
        };

        if x_group.refs.len() == y_group.refs.len() {
            let _ = self.ensure_spec_segment_bounds();
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
    ) -> IonResult<Option<ArrayWindow>> {
        let kept_segments = {
            let SegmentBoundsCache::Loaded(bounds) = &self.spec_segment_bounds else {
                return Ok(None);
            };
            let mut kept = Vec::with_capacity(x_group.refs.len());
            for segment_index in 0..x_group.refs.len() {
                let global_ref_index = x_ref_base + segment_index as u64;
                let Some((segment_low, segment_high)) = bounds.get(global_ref_index) else {
                    return Ok(None);
                };
                let overlaps_window = segment_low <= high && segment_high >= low;
                if overlaps_window {
                    kept.push(segment_index);
                }
            }
            kept
        };

        let mut x_out = Vec::new();
        let mut y_out = Vec::new();
        let mut x_segment = Vec::new();
        let mut y_segment = Vec::new();
        for segment_index in kept_segments {
            self.read_array(&x_group.refs[segment_index], &mut x_segment)?;
            self.read_array(&y_group.refs[segment_index], &mut y_segment)?;
            keep_pairs_in_range_sorted(&x_segment, &y_segment, low, high, &mut x_out, &mut y_out);
        }

        Ok(Some(ArrayWindow { x: x_out, y: y_out }))
    }

    pub(crate) fn read_full_window(
        &mut self,
        x_group: &ArrayGroup,
        y_group: &ArrayGroup,
        low: f64,
        high: f64,
    ) -> IonResult<ArrayWindow> {
        let mut x = Vec::new();
        let mut y = Vec::new();
        self.read_group_values(x_group, &mut x)?;
        self.read_group_values(y_group, &mut y)?;

        let mut x_out = Vec::new();
        let mut y_out = Vec::new();
        keep_pairs_in_range(&x, &y, low, high, &mut x_out, &mut y_out);
        Ok(ArrayWindow { x: x_out, y: y_out })
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

        let (entries_buf, array_refs_buf, segment_bounds) = match target {
            Target::Spec => {
                let _ = self.ensure_spec_segment_bounds();
                (
                    &self.spec_entries_buf,
                    &self.spec_array_refs,
                    &self.spec_segment_bounds,
                )
            }
            Target::Chrom => {
                self.ensure_chrom_segment_bounds();
                (
                    &self.chrom_entries_buf,
                    &self.chrom_array_refs,
                    &self.chrom_segment_bounds,
                )
            }
        };

        let item_count = match target {
            Target::Spec => self.header.spectrum_count,
            Target::Chrom => self.header.chrom_count,
        };

        let bounds = match segment_bounds {
            SegmentBoundsCache::Loaded(b) => Some(b),
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
                        if let Some((segment_low, segment_high)) = b.get(array_ref_index) {
                            segment_low <= hi && segment_high >= lo
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
