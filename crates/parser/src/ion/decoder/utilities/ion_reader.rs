use crate::encoder::encode::FILTER_INDEX_RECORD_SIZE;
use crate::ion::encoder::utilities::container_builder::FilterType;
use crate::ion::utilities::spectrum_source::ScanMeta;
use crate::ion::utilities::{
    container_view::{ContainerView, DefaultProcessor},
    parse_header::{Header, parse_header},
    spectrum_source::{SpectrumSource, f16_bits_to_f64},
};
use crate::mzml::structs::FilterRecord;

const ACC_MZ: u32 = 1_000_514;
const ACC_INT: u32 = 1_000_515;
const ENTRY_A_BYTES: usize = 16;
const ENTRY_A1_BYTES: usize = 32;

#[derive(Debug, Clone, Copy)]
pub struct ArrayRef {
    pub block_id: u32,
    pub element_offset: u64,
    pub element_count: u64,
    pub array_type: u32,
    pub dtype: u8,
}

pub struct IonReader<'a> {
    bytes: &'a [u8],
    header: Header,
    container: ContainerView<'a, DefaultProcessor>,
    mz_buf: Vec<f64>,
    int_buf: Vec<f64>,
}

impl<'a> IonReader<'a> {
    pub fn open(bytes: &'a [u8]) -> Result<Self, String> {
        let header = parse_header(bytes)?;
        let filter = FilterType::try_from(header.array_filter).unwrap_or(FilterType::None);

        let off = header.off_container_spect as usize;
        let len = header.len_container_spect as usize;
        let container_bytes = bytes
            .get(off..off + len)
            .ok_or("spectrum container out of bounds")?;

        let container = ContainerView::new(
            container_bytes,
            header.block_count_spect,
            header.compression_level,
            filter,
            "spec",
            DefaultProcessor,
        )?;

        Ok(Self {
            bytes,
            header,
            container,
            mz_buf: Vec::new(),
            int_buf: Vec::new(),
        })
    }

    #[inline]
    pub fn spectrum_count(&self) -> u64 {
        self.header.spectrum_count
    }

    pub fn filter_record(&self, index: usize) -> Option<FilterRecord> {
        let base = self.header.off_filter_index as usize + index * FILTER_INDEX_RECORD_SIZE;
        let b = self.bytes.get(base..base + FILTER_INDEX_RECORD_SIZE)?;
        Some(FilterRecord {
            rt_seconds: f64::from_le_bytes(b[0..8].try_into().unwrap()),
            base_peak_mz: f64::from_le_bytes(b[8..16].try_into().unwrap()),
            selected_ion_mz: f64::from_le_bytes(b[16..24].try_into().unwrap()),
            base_peak_int: f64::from_le_bytes(b[24..32].try_into().unwrap()),
            total_ion_current: f64::from_le_bytes(b[32..40].try_into().unwrap()),
            ms_level: b[40],
            polarity: b[41],
        })
    }

    pub fn array_refs_for_spectrum(&self, index: usize) -> Option<Vec<ArrayRef>> {
        if index >= self.header.spectrum_count as usize {
            return None;
        }
        let ea = self.header.off_spec_entries as usize + index * ENTRY_A_BYTES;
        let entry = self.bytes.get(ea..ea + ENTRY_A_BYTES)?;
        let ref_start = u64::from_le_bytes(entry[0..8].try_into().unwrap()) as usize;
        let ref_count = u64::from_le_bytes(entry[8..16].try_into().unwrap()) as usize;
        let aref_base = self.header.off_spec_arrayrefs as usize;

        let mut refs = Vec::with_capacity(ref_count);
        for j in 0..ref_count {
            let ab = aref_base + (ref_start + j) * ENTRY_A1_BYTES;
            let b = self.bytes.get(ab..ab + ENTRY_A1_BYTES)?;
            refs.push(parse_array_ref(b));
        }
        Some(refs)
    }

    pub fn read_array(&mut self, aref: &ArrayRef) -> Result<Vec<f64>, String> {
        let stride = dtype_stride(aref.dtype);
        let raw = self.container.get_item_from_block(
            aref.block_id,
            aref.element_offset,
            aref.element_count,
            stride,
            "read_array",
        )?;
        let mut buf = Vec::new();
        decode_into(&mut buf, raw, aref.dtype);
        Ok(buf)
    }
}

impl<'a> SpectrumSource for IonReader<'a> {
    fn for_each_scan_in_range(
        &mut self,
        rt_min: f64,
        rt_max: f64,
        ms_level: u8,
        callback: &mut dyn FnMut(f64, &ScanMeta, &[f64], &[f64]),
    ) {
        let rt_min_s = rt_min * 60.0;
        let rt_max_s = rt_max * 60.0;
        let count = self.header.spectrum_count as usize;
        let filter_base = self.header.off_filter_index as usize;
        let entry_base = self.header.off_spec_entries as usize;
        let aref_base = self.header.off_spec_arrayrefs as usize;

        for i in 0..count {
            let fb = filter_base + i * FILTER_INDEX_RECORD_SIZE;
            let Some(fs) = self.bytes.get(fb..fb + FILTER_INDEX_RECORD_SIZE) else {
                continue;
            };

            let rt_s = f64::from_le_bytes(fs[0..8].try_into().unwrap());
            if !rt_s.is_finite() || rt_s < rt_min_s || rt_s > rt_max_s {
                continue;
            }

            let ms = fs[40];
            if ms_level != 0 && ms != ms_level {
                continue;
            }

            let ea = entry_base + i * ENTRY_A_BYTES;
            let Some(es) = self.bytes.get(ea..ea + ENTRY_A_BYTES) else {
                continue;
            };
            let ref_start = u64::from_le_bytes(es[0..8].try_into().unwrap()) as usize;
            let ref_count = u64::from_le_bytes(es[8..16].try_into().unwrap()) as usize;

            let mut mz_ref: Option<ArrayRef> = None;
            let mut int_ref: Option<ArrayRef> = None;

            for j in 0..ref_count {
                let ab = aref_base + (ref_start + j) * ENTRY_A1_BYTES;
                let Some(ar) = self.bytes.get(ab..ab + ENTRY_A1_BYTES) else {
                    break;
                };
                let aref = parse_array_ref(ar);
                match aref.array_type {
                    ACC_MZ => mz_ref = Some(aref),
                    ACC_INT => int_ref = Some(aref),
                    _ => {}
                }
                if mz_ref.is_some() && int_ref.is_some() {
                    break;
                }
            }

            let (Some(mr), Some(ir)) = (mz_ref, int_ref) else {
                continue;
            };
            if !decode_from_block(&mut self.container, &mut self.mz_buf, &mr) {
                continue;
            }
            if !decode_from_block(&mut self.container, &mut self.int_buf, &ir) {
                continue;
            }

            let n = self.mz_buf.len().min(self.int_buf.len());
            if n == 0 {
                continue;
            }

            let meta = ScanMeta {
                ms_level: ms,
                polarity: fs[41],
                base_peak_mz: f64::from_le_bytes(fs[8..16].try_into().unwrap()),
                selected_ion_mz: f64::from_le_bytes(fs[16..24].try_into().unwrap()),
                base_peak_int: f64::from_le_bytes(fs[24..32].try_into().unwrap()),
                total_ion_current: f64::from_le_bytes(fs[32..40].try_into().unwrap()),
            };

            callback(rt_s / 60.0, &meta, &self.mz_buf[..n], &self.int_buf[..n]);
        }
    }
}

#[inline]
fn parse_array_ref(b: &[u8]) -> ArrayRef {
    ArrayRef {
        element_offset: u64::from_le_bytes(b[0..8].try_into().unwrap()),
        element_count: u64::from_le_bytes(b[8..16].try_into().unwrap()),
        block_id: u32::from_le_bytes(b[16..20].try_into().unwrap()),
        array_type: u32::from_le_bytes(b[20..24].try_into().unwrap()),
        dtype: b[24],
    }
}

#[inline]
fn decode_from_block(
    container: &mut ContainerView<'_, DefaultProcessor>,
    buf: &mut Vec<f64>,
    aref: &ArrayRef,
) -> bool {
    let stride = dtype_stride(aref.dtype);
    let raw = match container.get_item_from_block(
        aref.block_id,
        aref.element_offset,
        aref.element_count,
        stride,
        "scan",
    ) {
        Ok(r) => r,
        Err(_) => return false,
    };
    decode_into(buf, raw, aref.dtype);
    true
}

#[inline]
fn dtype_stride(dtype: u8) -> usize {
    match dtype {
        1 | 6 => 8,
        2 | 5 => 4,
        3 | 4 => 2,
        _ => 1,
    }
}

fn decode_into(buf: &mut Vec<f64>, raw: &[u8], dtype: u8) {
    buf.clear();
    match dtype {
        1 => {
            let n = raw.len() / 8;
            buf.reserve(n);
            unsafe {
                buf.set_len(n);
                std::ptr::copy_nonoverlapping(raw.as_ptr(), buf.as_mut_ptr() as *mut u8, raw.len());
            }
        }
        2 => buf.extend(
            raw.chunks_exact(4)
                .map(|c| f32::from_le_bytes(c.try_into().unwrap()) as f64),
        ),
        3 => buf.extend(
            raw.chunks_exact(2)
                .map(|c| f16_bits_to_f64(u16::from_le_bytes(c.try_into().unwrap()))),
        ),
        4 => buf.extend(
            raw.chunks_exact(2)
                .map(|c| i16::from_le_bytes(c.try_into().unwrap()) as f64),
        ),
        5 => buf.extend(
            raw.chunks_exact(4)
                .map(|c| i32::from_le_bytes(c.try_into().unwrap()) as f64),
        ),
        6 => buf.extend(
            raw.chunks_exact(8)
                .map(|c| i64::from_le_bytes(c.try_into().unwrap()) as f64),
        ),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BYTES: &[u8] = include_bytes!("../../../../data/ion/test.ion");

    #[test]
    fn open_parses_header() {
        let reader = IonReader::open(BYTES).unwrap();
        assert!(reader.spectrum_count() > 0);
    }

    #[test]
    fn filter_record_returns_none_out_of_bounds() {
        let reader = IonReader::open(BYTES).unwrap();
        assert!(
            reader
                .filter_record(reader.spectrum_count() as usize)
                .is_none()
        );
    }

    #[test]
    fn filter_record_has_valid_rt() {
        let reader = IonReader::open(BYTES).unwrap();
        let r = reader.filter_record(0).unwrap();
        assert!(r.rt_seconds.is_finite() && r.rt_seconds >= 0.0);
        assert!(r.ms_level >= 1);
    }

    #[test]
    fn array_refs_contain_mz_and_intensity() {
        let reader = IonReader::open(BYTES).unwrap();
        let refs = reader.array_refs_for_spectrum(0).unwrap();
        assert!(refs.iter().any(|a| a.array_type == ACC_MZ));
        assert!(refs.iter().any(|a| a.array_type == ACC_INT));
    }

    #[test]
    fn read_array_produces_mz_values() {
        let mut reader = IonReader::open(BYTES).unwrap();
        let refs = reader.array_refs_for_spectrum(0).unwrap();
        let mz_ref = refs.iter().find(|a| a.array_type == ACC_MZ).unwrap();
        let mz = reader.read_array(mz_ref).unwrap();
        assert!(!mz.is_empty());
        assert!(mz.iter().all(|v| v.is_finite()));
    }

    #[test]
    fn for_each_scan_yields_matching_scans() {
        let mut reader = IonReader::open(BYTES).unwrap();
        let mut count = 0usize;
        reader.for_each_scan_in_range(0.0, f64::MAX, 0, &mut |rt, _meta, mz, int| {
            assert!(rt.is_finite());
            assert!(!mz.is_empty());
            assert_eq!(mz.len(), int.len());
            count += 1;
        });
        assert_eq!(count, reader.spectrum_count() as usize);
    }

    #[test]
    fn for_each_scan_filters_by_ms_level() {
        let mut reader = IonReader::open(BYTES).unwrap();
        let mut count = 0usize;
        reader.for_each_scan_in_range(0.0, f64::MAX, 1, &mut |_, _, _, _| {
            count += 1;
        });
        let expected = (0..reader.spectrum_count() as usize)
            .filter(|&i| reader.filter_record(i).map_or(false, |r| r.ms_level == 1))
            .count();
        assert_eq!(count, expected);
    }

    #[test]
    fn decode_into_f64_roundtrips() {
        let vals = [1.5f64, 2.5, 3.5];
        let raw: Vec<u8> = vals.iter().flat_map(|v| v.to_le_bytes()).collect();
        let mut buf = Vec::new();
        decode_into(&mut buf, &raw, 1);
        assert_eq!(buf, vals);
    }

    #[test]
    fn decode_into_f32_converts() {
        let vals = [1.0f32, 2.0];
        let raw: Vec<u8> = vals.iter().flat_map(|v| v.to_le_bytes()).collect();
        let mut buf = Vec::new();
        decode_into(&mut buf, &raw, 2);
        assert_eq!(buf.len(), 2);
        assert!((buf[0] - 1.0).abs() < f64::EPSILON);
        assert!((buf[1] - 2.0).abs() < f64::EPSILON);
    }

    #[test]
    fn decode_into_reuses_buffer() {
        let raw = 42.0f64.to_le_bytes();
        let mut buf = Vec::with_capacity(1024);
        decode_into(&mut buf, &raw, 1);
        assert_eq!(buf.len(), 1);
        let cap = buf.capacity();
        decode_into(&mut buf, &raw, 1);
        assert_eq!(buf.capacity(), cap);
    }

    #[test]
    fn dtype_stride_maps_all_types() {
        assert_eq!(dtype_stride(1), 8);
        assert_eq!(dtype_stride(2), 4);
        assert_eq!(dtype_stride(3), 2);
        assert_eq!(dtype_stride(4), 2);
        assert_eq!(dtype_stride(5), 4);
        assert_eq!(dtype_stride(6), 8);
    }
}
