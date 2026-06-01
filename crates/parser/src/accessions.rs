pub(crate) const MZ_ARRAY: u32 = 1_000_514;
pub(crate) const INTENSITY_ARRAY: u32 = 1_000_515;
pub(crate) const TIME_ARRAY: u32 = 1_000_595;
pub(crate) const FLOAT_16BIT: u32 = 1_000_520;
pub(crate) const FLOAT_32BIT: u32 = 1_000_521;
pub(crate) const FLOAT_64BIT: u32 = 1_000_523;
pub(crate) const INT_16BIT: u32 = 1_000_518;
pub(crate) const INT_32BIT: u32 = 1_000_519;
pub(crate) const INT_64BIT: u32 = 1_000_522;
pub(crate) const SCAN_START_TIME: u32 = 1_000_016;
pub(crate) const MS_LEVEL: u32 = 1_000_511;
pub(crate) const BASE_PEAK_MZ: u32 = 1_000_504;
pub(crate) const BASE_PEAK_INT: u32 = 1_000_505;
pub(crate) const TOTAL_ION_CURRENT: u32 = 1_000_285;
pub(crate) const SELECTED_ION_MZ: u32 = 1_000_744;
pub(crate) const POSITIVE_SCAN: u32 = 1_000_130;
pub(crate) const NEGATIVE_SCAN: u32 = 1_000_129;
pub(crate) const UNIT_MINUTE: u32 = 31;
pub(crate) const UNIT_SECOND: u32 = 10;
pub(crate) const UNIT_MS: u32 = 28;
pub(crate) const HIGHEST_OBSERVED_MZ: u32 = 1_000_527;
pub(crate) const LOWEST_OBSERVED_MZ: u32 = 1_000_528;
pub(crate) const HIGHEST_OBSERVED_WAVELENGTH: u32 = 1_000_618;
pub(crate) const LOWEST_OBSERVED_WAVELENGTH: u32 = 1_000_619;
pub(crate) const LOWEST_ION_MOBILITY: u32 = 1_003_437;
pub(crate) const HIGHEST_ION_MOBILITY: u32 = 1_003_438;
pub(crate) const ION_MOBILITY_ARRAY: u32 = 1_002_893;
pub(crate) const MEAN_ION_MOBILITY_ARRAY: u32 = 1_002_816;
pub(crate) const RAW_ION_MOBILITY_ARRAY: u32 = 1_003_007;
pub(crate) const RAW_ION_MOBILITY_DRIFT_TIME_ARRAY: u32 = 1_003_153;

pub(crate) const ACC_MS_LEVEL: &str = "MS:1000511";

#[inline]
pub(crate) fn format_accession(tail: u32) -> String {
    format!("MS:{tail:07}")
}
pub(crate) const ACC_SOURCE_ESI: &str = "MS:1000073";
pub(crate) const ACC_SOURCE_EI: &str = "MS:1000057";
pub(crate) const ACC_ANALYZER_QUAD: &str = "MS:1000081";
pub(crate) const ACC_ANALYZER_TOF: &str = "MS:1000084";
pub(crate) const ACC_DETECTOR_EM: &str = "MS:1000114";
pub(crate) const ACC_DETECTOR_PHOTOMULT: &str = "MS:1000116";
pub(crate) const ACC_COMPRESSION_ZLIB: &str = "MS:1000574";
pub(crate) const ACC_COMPRESSION_NONE: &str = "MS:1000576";
pub(crate) const ACC_FLOAT_16BIT_STR: &str = "MS:1000520";
pub(crate) const ACC_FLOAT_32BIT_STR: &str = "MS:1000521";
pub(crate) const ACC_FLOAT_64BIT_STR: &str = "MS:1000523";
pub(crate) const ACC_INT_16BIT_STR: &str = "MS:1000518";
pub(crate) const ACC_INT_32BIT_STR: &str = "MS:1000519";
pub(crate) const ACC_INT_64BIT_STR: &str = "MS:1000522";
