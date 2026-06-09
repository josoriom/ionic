use crate::{
    ion::utilities::{
        children_lookup::{ChildrenLookup, OwnerRows},
        parse_scan_list,
    },
    mzml::{schema::TagId, structs::CvParam},
};

const PATH: &str = "data/ion/test.ion";

#[allow(clippy::too_many_arguments)] //TODO: Need to fix this
fn assert_cv_param(
    p: &CvParam,
    cv_ref: Option<&str>,
    accession: Option<&str>,
    name: &str,
    value: Option<&str>,
    unit_cv_ref: Option<&str>,
    unit_name: Option<&str>,
    unit_accession: Option<&str>,
) {
    assert_eq!(p.cv_ref.as_deref(), cv_ref);
    assert_eq!(p.accession.as_deref(), accession);
    assert_eq!(p.name.as_str(), name);
    assert_eq!(p.value.as_deref(), value);
    assert_eq!(p.unit_cv_ref.as_deref(), unit_cv_ref);
    assert_eq!(p.unit_name.as_deref(), unit_name);
    assert_eq!(p.unit_accession.as_deref(), unit_accession);
}

#[test]
fn first_spectrum_scan_list_cv_params_item_by_item() {
    let metadata_section = super::meta::spectra_metadata(PATH);

    let mut rows_by_id = OwnerRows::with_capacity(metadata_section.len());
    for metadatum in &metadata_section {
        rows_by_id.insert(metadatum.id, metadatum);
    }

    let spectrum_id = metadata_section
        .iter()
        .find(|metadatum| metadatum.tag_id == TagId::Spectrum)
        .map(|metadatum| metadatum.id)
        .expect("no Spectrum entries found in spectra metadata");

    let children_lookup = ChildrenLookup::new(&metadata_section);

    let scan_list = parse_scan_list(&rows_by_id, &children_lookup, spectrum_id)
        .expect("parse_scan_list returned None");
    assert_eq!(scan_list.count, Some(1));
    assert_eq!(scan_list.scans.len(), 1);

    let scan = &scan_list.scans[0];

    let p = scan
        .cv_params
        .iter()
        .find(|p| p.accession.as_deref() == Some("MS:1000016"))
        .expect("missing MS:1000016 (scan start time)");
    assert_cv_param(
        p,
        Some("MS"),
        Some("MS:1000016"),
        "scan start time",
        Some("0.191"),
        Some("UO"),
        Some("second"),
        Some("UO:0000010"),
    );

    let swl = scan
        .scan_window_list
        .as_ref()
        .expect("missing scanWindowList");
    assert_eq!(swl.count, Some(1));
    assert_eq!(swl.scan_windows.len(), 1);

    let sw = &swl.scan_windows[0];

    let p = sw
        .cv_params
        .iter()
        .find(|p| p.accession.as_deref() == Some("MS:1000501"))
        .expect("missing MS:1000501 (scan window lower limit)");
    assert_cv_param(
        p,
        Some("MS"),
        Some("MS:1000501"),
        "scan window lower limit",
        Some("30"),
        Some("MS"),
        Some("m/z"),
        Some("MS:1000040"),
    );

    let p = sw
        .cv_params
        .iter()
        .find(|p| p.accession.as_deref() == Some("MS:1000500"))
        .expect("missing MS:1000500 (scan window upper limit)");
    assert_cv_param(
        p,
        Some("MS"),
        Some("MS:1000500"),
        "scan window upper limit",
        Some("1000"),
        Some("MS"),
        Some("m/z"),
        Some("MS:1000040"),
    );
}

#[test]
fn second_spectrum_scan_list_cv_params_item_by_item() {
    let metadata_section = super::meta::spectra_metadata(PATH);

    let mut rows_by_id = OwnerRows::with_capacity(metadata_section.len());
    for metadatum in &metadata_section {
        rows_by_id.insert(metadatum.id, metadatum);
    }

    let mut spectrum_item_indices: Vec<_> = metadata_section
        .iter()
        .filter(|m| m.tag_id == TagId::Spectrum)
        .map(|m| m.item_index)
        .collect();
    spectrum_item_indices.sort_unstable();
    spectrum_item_indices.dedup();

    let target_item_index = spectrum_item_indices
        .get(1)
        .copied()
        .expect("no second Spectrum item_index found");

    let spectrum_id = metadata_section
        .iter()
        .find(|m| m.tag_id == TagId::Spectrum && m.item_index == target_item_index)
        .map(|m| m.id)
        .expect("target spectrum ID not found");

    let children_lookup = ChildrenLookup::new(&metadata_section);

    let scan_list = parse_scan_list(&rows_by_id, &children_lookup, spectrum_id)
        .expect("parse_scan_list returned None");

    assert_eq!(scan_list.count, Some(1));
    assert_eq!(scan_list.scans.len(), 1);

    let scan = &scan_list.scans[0];
    let p = scan
        .cv_params
        .iter()
        .find(|p| p.accession.as_deref() == Some("MS:1000016"))
        .expect("missing MS:1000016 (scan start time)");
    assert_cv_param(
        p,
        Some("MS"),
        Some("MS:1000016"),
        "scan start time",
        Some("452.262"),
        Some("UO"),
        Some("second"),
        Some("UO:0000010"),
    );

    let swl = scan
        .scan_window_list
        .as_ref()
        .expect("missing scanWindowList");
    assert_eq!(swl.count, Some(1));
    assert_eq!(swl.scan_windows.len(), 1);

    let sw = &swl.scan_windows[0];

    let p = sw
        .cv_params
        .iter()
        .find(|p| p.accession.as_deref() == Some("MS:1000501"))
        .expect("missing MS:1000501 (scan window lower limit)");
    assert_cv_param(
        p,
        Some("MS"),
        Some("MS:1000501"),
        "scan window lower limit",
        Some("30"),
        Some("MS"),
        Some("m/z"),
        Some("MS:1000040"),
    );

    let p = sw
        .cv_params
        .iter()
        .find(|p| p.accession.as_deref() == Some("MS:1000500"))
        .expect("missing MS:1000500 (scan window upper limit)");
    assert_cv_param(
        p,
        Some("MS"),
        Some("MS:1000500"),
        "scan window upper limit",
        Some("1000"),
        Some("MS"),
        Some("m/z"),
        Some("MS:1000040"),
    );
}
