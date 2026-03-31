use crate::mzml::structs::{BinaryData, MzML, Spectrum};
use std::borrow::Cow;

const ACC_SCAN_START_TIME: &str = "MS:1000016";
const ACC_MS_LEVEL: &str = "MS:1000511";
const ACC_MZ_ARRAY: &str = "MS:1000514";
const ACC_INTENSITY_ARRAY: &str = "MS:1000515";
const UO_MINUTE: &str = "UO:0000031";
const UO_SECOND: &str = "UO:0000010";
const UO_MILLISECOND: &str = "UO:0000028";

#[derive(Debug, Clone, Copy)]
pub struct ScanMeta {
    pub ms_level: u8,
    pub polarity: u8,
    pub base_peak_mz: f64,
    pub selected_ion_mz: f64,
    pub base_peak_int: f64,
    pub total_ion_current: f64,
}

pub trait SpectrumSource {
    fn for_each_scan_in_range(
        &mut self,
        rt_min: f64,
        rt_max: f64,
        ms_level: u8,
        callback: &mut dyn FnMut(f64, &ScanMeta, &[f64], &[f64]),
    );
}

impl SpectrumSource for MzML {
    fn for_each_scan_in_range(
        &mut self,
        rt_min: f64,
        rt_max: f64,
        ms_level: u8,
        callback: &mut dyn FnMut(f64, &ScanMeta, &[f64], &[f64]),
    ) {
        let spectra = match self.run.spectrum_list.as_ref() {
            Some(sl) => sl.spectra.as_slice(),
            None => return,
        };

        for spectrum in spectra.iter() {
            let level = match extract_ms_level(spectrum) {
                Some(l) => l,
                None => continue,
            };
            if ms_level != 0 && level != ms_level {
                continue;
            }
            let rt = match extract_rt_minutes(spectrum) {
                Some(rt) if rt >= rt_min && rt <= rt_max => rt,
                _ => continue,
            };
            let meta = ScanMeta {
                ms_level: level,
                polarity: 0,
                base_peak_mz: f64::NAN,
                selected_ion_mz: f64::NAN,
                base_peak_int: f64::NAN,
                total_ion_current: f64::NAN,
            };
            let Some((mz_data, int_data)) = extract_binary_pair(spectrum) else {
                continue;
            };
            let mz = as_f64_cow(mz_data);
            let intensity = as_f64_cow(int_data);
            let n = mz.len().min(intensity.len());
            if n > 0 {
                callback(rt, &meta, &mz[..n], &intensity[..n]);
            }
        }
    }
}

pub(crate) fn extract_ms_level(spectrum: &Spectrum) -> Option<u8> {
    if let Some(level) = spectrum.ms_level {
        return u8::try_from(level).ok();
    }
    spectrum
        .cv_params
        .iter()
        .find(|p| p.accession.as_deref() == Some(ACC_MS_LEVEL))
        .and_then(|p| p.value.as_deref()?.parse::<u8>().ok())
}

pub(crate) fn extract_rt_minutes(spectrum: &Spectrum) -> Option<f64> {
    let scan_list = spectrum.scan_list.as_ref().or_else(|| {
        spectrum
            .spectrum_description
            .as_ref()
            .and_then(|d| d.scan_list.as_ref())
    })?;
    for scan in &scan_list.scans {
        for p in &scan.cv_params {
            if p.accession.as_deref() == Some(ACC_SCAN_START_TIME) {
                let raw: f64 = p.value.as_deref()?.parse().ok()?;
                return to_minutes(raw, p.unit_accession.as_deref(), p.unit_name.as_deref());
            }
        }
    }
    for p in &scan_list.cv_params {
        if p.accession.as_deref() == Some(ACC_SCAN_START_TIME) {
            let raw: f64 = p.value.as_deref()?.parse().ok()?;
            return to_minutes(raw, p.unit_accession.as_deref(), p.unit_name.as_deref());
        }
    }
    None
}

fn to_minutes(val: f64, unit_acc: Option<&str>, unit_name: Option<&str>) -> Option<f64> {
    if !val.is_finite() {
        return None;
    }
    match unit_acc {
        Some(UO_MINUTE) => Some(val),
        Some(UO_SECOND) => Some(val / 60.0),
        Some(UO_MILLISECOND) => Some(val / 60_000.0),
        _ => match unit_name {
            Some("minute" | "minutes") => Some(val),
            Some("second" | "seconds") => Some(val / 60.0),
            Some("millisecond" | "milliseconds") => Some(val / 60_000.0),
            _ => None,
        },
    }
}

pub(crate) fn extract_binary_pair(spectrum: &Spectrum) -> Option<(&BinaryData, &BinaryData)> {
    let list = spectrum.binary_data_array_list.as_ref()?;
    let mz = list
        .binary_data_arrays
        .iter()
        .find(|a| {
            a.cv_params
                .iter()
                .any(|p| p.accession.as_deref() == Some(ACC_MZ_ARRAY))
        })?
        .binary
        .as_ref()?;
    let int = list
        .binary_data_arrays
        .iter()
        .find(|a| {
            a.cv_params
                .iter()
                .any(|p| p.accession.as_deref() == Some(ACC_INTENSITY_ARRAY))
        })?
        .binary
        .as_ref()?;
    Some((mz, int))
}

pub(crate) fn as_f64_cow(data: &BinaryData) -> Cow<'_, [f64]> {
    match data {
        BinaryData::F64(v) => Cow::Borrowed(v),
        BinaryData::F32(v) => Cow::Owned(v.iter().map(|&x| x as f64).collect()),
        BinaryData::I16(v) => Cow::Owned(v.iter().map(|&x| x as f64).collect()),
        BinaryData::I32(v) => Cow::Owned(v.iter().map(|&x| x as f64).collect()),
        BinaryData::I64(v) => Cow::Owned(v.iter().map(|&x| x as f64).collect()),
        BinaryData::F16(v) => Cow::Owned(v.iter().map(|&x| f16_bits_to_f64(x)).collect()),
    }
}

pub(crate) fn f16_bits_to_f64(bits: u16) -> f64 {
    let sign = ((bits & 0x8000) as u32) << 16;
    let exp = (bits & 0x7C00) >> 10;
    let mant = (bits & 0x03FF) as u32;
    let bits32 = match exp {
        0 if mant == 0 => sign,
        0 => {
            let mut m = mant;
            let mut e = 0u32;
            while m & 0x400 == 0 {
                m <<= 1;
                e += 1;
            }
            sign | ((127 - 14 - e) << 23) | ((m & 0x3FF) << 13)
        }
        31 => sign | 0x7F80_0000 | (mant << 13),
        e => sign | ((e as u32 + 112) << 23) | (mant << 13),
    };
    f32::from_bits(bits32) as f64
}
