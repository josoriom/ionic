mod common;

use common::assertions::*;
use common::test_files;
use common::{
    BinaryDataExt, cv_param_by_accession, cv_value_f64, find_array_by_accession,
    parse_scan_number_from_id, precursor_list_of_spectrum, scan_start_time_seconds,
    spectrum_arrays, spectrum_by_id,
};

#[test]
fn tiny_11_known_values_scan_19() {
    let mzml = test_files::tiny_pwiz_11();
    let s = spectrum_by_id(mzml, "scan=19");
    assert_eq!(s.ms_level, Some(1));
    assert_eq!(s.default_array_length, Some(15));
    let arrays = spectrum_arrays(s);
    let mz = find_array_by_accession(arrays, "MS:1000514");
    let intensity = find_array_by_accession(arrays, "MS:1000515");
    let mz_first = mz
        .binary
        .as_ref()
        .expect("mz binary present")
        .first_f64()
        .unwrap();
    let i_first = intensity
        .binary
        .as_ref()
        .expect("intensity binary present")
        .first_f64()
        .unwrap();
    rel_close_f64(mz_first, 0.0, EPS_REL_F64, "scan=19 first m/z");
    rel_close_f64(i_first, 15.0, EPS_REL_F64, "scan=19 first intensity");
}

#[test]
fn tiny_11_known_values_scan_20() {
    let mzml = test_files::tiny_pwiz_11();
    let s = spectrum_by_id(mzml, "scan=20");
    assert_eq!(s.ms_level, Some(2));
    assert_eq!(s.default_array_length, Some(10));
    let arrays = spectrum_arrays(s);
    let mz = find_array_by_accession(arrays, "MS:1000514");
    let intensity = find_array_by_accession(arrays, "MS:1000515");
    let mz_first = mz
        .binary
        .as_ref()
        .expect("mz binary present")
        .first_f64()
        .unwrap();
    let i_first = intensity
        .binary
        .as_ref()
        .expect("intensity binary present")
        .first_f64()
        .unwrap();
    rel_close_f64(mz_first, 0.0, EPS_REL_F64, "scan=20 first m/z");
    rel_close_f64(i_first, 20.0, EPS_REL_F64, "scan=20 first intensity");
}

#[test]
fn tiny_11_scan19_metadata_equivalence() {
    let s19 = spectrum_by_id(test_files::tiny_pwiz_11(), "scan=19");
    assert_eq!(parse_scan_number_from_id(&s19.id), Some(19));
    assert_eq!(s19.ms_level, Some(1));
    let rt_s = scan_start_time_seconds(s19).expect("scan start time");
    let mz_low = cv_value_f64(&s19.cv_params, "MS:1000528").expect("lowest observed m/z");
    let mz_high = cv_value_f64(&s19.cv_params, "MS:1000527").expect("highest observed m/z");
    rel_close_f64(rt_s, 353.43, 1e-6, "scan=19 RT (seconds)");
    rel_close_f64(mz_low, 400.39, 1e-6, "scan=19 mzLow");
    rel_close_f64(mz_high, 1795.56, 1e-6, "scan=19 mzHigh");
}

#[test]
fn tiny_11_scan20_precursor_equivalence() {
    let s20 = spectrum_by_id(test_files::tiny_pwiz_11(), "scan=20");
    assert_eq!(parse_scan_number_from_id(&s20.id), Some(20));
    assert_eq!(s20.ms_level, Some(2));
    let precursor = precursor_list_of_spectrum(s20)
        .and_then(|pl| pl.precursors.first())
        .expect("scan=20 precursor");
    let selected_ion = precursor
        .selected_ion_list
        .as_ref()
        .and_then(|sil| sil.selected_ions.first())
        .expect("scan=20 selected ion");
    let mz = cv_value_f64(&selected_ion.cv_params, "MS:1000744").expect("selected ion m/z");
    let intensity = cv_value_f64(&selected_ion.cv_params, "MS:1000042").expect("peak intensity");
    let charge = cv_param_by_accession(&selected_ion.cv_params, "MS:1000041")
        .and_then(|p| p.value.as_deref())
        .and_then(|v| v.parse::<i32>().ok())
        .expect("charge state");
    rel_close_f64(mz, 445.34, 1e-6, "scan=20 precursor m/z");
    rel_close_f64(intensity, 120053.0, 1e-9, "scan=20 precursor intensity");
    assert_eq!(charge, 2);
}
