use crate::mzml::structs::{BinaryData, CvParam, MzML, Spectrum};
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
    #[allow(clippy::type_complexity)]
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
        if let Some(list) = self.run.spectrum_list.as_ref() {
            for_each_spectra_in_range(&list.spectra, rt_min, rt_max, ms_level, callback);
        }
    }
}

#[allow(clippy::type_complexity)]
pub(crate) fn for_each_spectra_in_range(
    spectra: &[Spectrum],
    rt_min: f64,
    rt_max: f64,
    ms_level: u8,
    callback: &mut dyn FnMut(f64, &ScanMeta, &[f64], &[f64]),
) {
    for spectrum in spectra {
        let level = match extract_ms_level(spectrum) {
            Some(level) => level,
            None => continue,
        };
        if ms_level != 0 && level != ms_level {
            continue;
        }

        let rt = match extract_rt_minutes(spectrum) {
            Some(rt) if rt >= rt_min && rt <= rt_max => rt,
            _ => continue,
        };

        let Some((mz_data, intensity_data)) = extract_binary_pair(spectrum) else {
            continue;
        };
        let mz = as_f64_cow(mz_data);
        let intensity = as_f64_cow(intensity_data);
        let len = mz.len().min(intensity.len());
        if len == 0 {
            continue;
        }

        let meta = ScanMeta {
            ms_level: level,
            polarity: 0,
            base_peak_mz: f64::NAN,
            selected_ion_mz: f64::NAN,
            base_peak_int: f64::NAN,
            total_ion_current: f64::NAN,
        };
        callback(rt, &meta, &mz[..len], &intensity[..len]);
    }
}

pub(crate) fn extract_ms_level(spectrum: &Spectrum) -> Option<u8> {
    if let Some(level) = spectrum.ms_level {
        return u8::try_from(level).ok();
    }
    spectrum
        .cv_params
        .iter()
        .find(|param| param.accession.as_deref() == Some(ACC_MS_LEVEL))
        .and_then(|param| param.value.as_deref()?.parse::<u8>().ok())
}

pub(crate) fn extract_rt_minutes(spectrum: &Spectrum) -> Option<f64> {
    let scan_list = spectrum.scan_list.as_ref().or_else(|| {
        spectrum
            .spectrum_description
            .as_ref()
            .and_then(|description| description.scan_list.as_ref())
    })?;

    for scan in &scan_list.scans {
        if let Some(rt) = extract_rt_from_params(&scan.cv_params) {
            return Some(rt);
        }
    }

    extract_rt_from_params(&scan_list.cv_params)
}

#[inline]
fn extract_rt_from_params(params: &[CvParam]) -> Option<f64> {
    for param in params {
        if param.accession.as_deref() == Some(ACC_SCAN_START_TIME) {
            let value: f64 = param.value.as_deref()?.parse().ok()?;
            return to_minutes(
                value,
                param.unit_accession.as_deref(),
                param.unit_name.as_deref(),
            );
        }
    }
    None
}

fn to_minutes(value: f64, unit_accession: Option<&str>, unit_name: Option<&str>) -> Option<f64> {
    if !value.is_finite() {
        return None;
    }
    match unit_accession {
        Some(UO_MINUTE) => Some(value),
        Some(UO_SECOND) => Some(value / 60.0),
        Some(UO_MILLISECOND) => Some(value / 60_000.0),
        _ => match unit_name {
            Some("minute" | "minutes") => Some(value),
            Some("second" | "seconds") => Some(value / 60.0),
            Some("millisecond" | "milliseconds") => Some(value / 60_000.0),
            _ => None,
        },
    }
}

pub(crate) fn extract_binary_pair(spectrum: &Spectrum) -> Option<(&BinaryData, &BinaryData)> {
    let list = spectrum.binary_data_array_list.as_ref()?;
    let mut mz = None;
    let mut intensity = None;

    for array in &list.binary_data_arrays {
        if mz.is_some() && intensity.is_some() {
            break;
        }
        let mut is_mz = false;
        let mut is_intensity = false;
        for param in &array.cv_params {
            match param.accession.as_deref() {
                Some(ACC_MZ_ARRAY) => is_mz = true,
                Some(ACC_INTENSITY_ARRAY) => is_intensity = true,
                _ => {}
            }
            if is_mz && is_intensity {
                break;
            }
        }
        if is_mz {
            mz = array.binary.as_ref();
        }
        if is_intensity {
            intensity = array.binary.as_ref();
        }
    }

    Some((mz?, intensity?))
}

pub(crate) fn as_f64_cow(data: &BinaryData) -> Cow<'_, [f64]> {
    match data {
        BinaryData::F64(values) => Cow::Borrowed(values),
        BinaryData::F32(values) => {
            Cow::Owned(map_to_f64(values.iter().copied().map(|value| value as f64)))
        }
        BinaryData::I16(values) => {
            Cow::Owned(map_to_f64(values.iter().copied().map(|value| value as f64)))
        }
        BinaryData::I32(values) => {
            Cow::Owned(map_to_f64(values.iter().copied().map(|value| value as f64)))
        }
        BinaryData::I64(values) => {
            Cow::Owned(map_to_f64(values.iter().copied().map(|value| value as f64)))
        }
        BinaryData::F16(values) => {
            Cow::Owned(map_to_f64(values.iter().copied().map(f16_bits_to_f64)))
        }
    }
}

#[inline]
fn map_to_f64<I>(iter: I) -> Vec<f64>
where
    I: Iterator<Item = f64> + ExactSizeIterator,
{
    let mut out = Vec::with_capacity(iter.len());
    out.extend(iter);
    out
}

pub(crate) fn f16_bits_to_f64(bits: u16) -> f64 {
    let sign = ((bits & 0x8000) as u32) << 16;
    let exp = (bits & 0x7C00) >> 10;
    let mant = (bits & 0x03FF) as u32;
    let bits32 = match exp {
        0 if mant == 0 => sign,
        0 => {
            let mut mantissa = mant;
            let mut exponent_shift = 0u32;
            while mantissa & 0x400 == 0 {
                mantissa <<= 1;
                exponent_shift += 1;
            }
            sign | ((127 - 14 - exponent_shift) << 23) | ((mantissa & 0x03FF) << 13)
        }
        31 => sign | 0x7F80_0000 | (mant << 13),
        exponent => sign | ((exponent as u32 + 112) << 23) | (mant << 13),
    };
    f32::from_bits(bits32) as f64
}
