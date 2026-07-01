use crate::{
    ion::utilities::{
        children_lookup::{ChildrenLookup, OwnerRows},
        parse_precursor_list,
    },
    mzml::{schema::TagId, structs::CvParam},
};

const PATH: &str = "data/ion/test.ion";

#[allow(clippy::too_many_arguments)]
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
fn first_spectrum_precursor_list_must_be_none() {
    let metadata_section = super::meta::spectra_metadata(PATH);

    let mut rows_by_id = OwnerRows::with_capacity(metadata_section.len());
    for metadatum in &metadata_section {
        rows_by_id.insert(metadatum.id, metadatum);
    }

    let spectrum_id = metadata_section
        .iter()
        .find(|metadatum| metadatum.tag_id == TagId::Spectrum)
        .map(|metadatum| metadatum.id)
        .expect("no Spectrum entries found");

    let children_lookup = ChildrenLookup::new(&metadata_section);

    let precursor_list = parse_precursor_list(&rows_by_id, &children_lookup, spectrum_id);

    assert!(
        precursor_list.is_none(),
        "first spectrum should not contain <precursorList>"
    );
}

#[test]
fn second_spectrum_precursor_list_cv_params_item_by_item() {
    let metadata_section = super::meta::spectra_metadata(PATH);

    let mut rows_by_id = OwnerRows::with_capacity(metadata_section.len());
    for metadatum in &metadata_section {
        rows_by_id.insert(metadatum.id, metadatum);
    }

    let mut spectrum_ids: Vec<u32> = metadata_section
        .iter()
        .filter(|metadatum| metadatum.tag_id == TagId::Spectrum)
        .map(|metadatum| metadatum.id)
        .collect();

    spectrum_ids.sort_unstable();
    spectrum_ids.dedup();

    let target_spectrum_id = spectrum_ids
        .get(1)
        .copied()
        .expect("no second Spectrum ID found");

    let children_lookup = ChildrenLookup::new(&metadata_section);

    let precursor_list = parse_precursor_list(&rows_by_id, &children_lookup, target_spectrum_id)
        .expect("parse_precursor_list returned None");

    assert_eq!(precursor_list.count, Some(1));
    assert_eq!(precursor_list.precursors.len(), 1);

    let precursor = &precursor_list.precursors[0];

    let iw = precursor
        .isolation_window
        .as_ref()
        .expect("missing isolationWindow");

    let p = iw
        .cv_params
        .iter()
        .find(|p| p.accession.as_deref() == Some("MS:1000827"))
        .expect("missing MS:1000827 (isolation window target m/z)");
    assert_cv_param(
        p,
        Some("MS"),
        Some("MS:1000827"),
        "isolation window target m/z",
        Some("515"),
        Some("MS"),
        Some("m/z"),
        Some("MS:1000040"),
    );

    let p = iw
        .cv_params
        .iter()
        .find(|p| p.accession.as_deref() == Some("MS:1000828"))
        .expect("missing MS:1000828 (isolation window lower offset)");
    assert_cv_param(
        p,
        Some("MS"),
        Some("MS:1000828"),
        "isolation window lower offset",
        Some("485"),
        Some("MS"),
        Some("m/z"),
        Some("MS:1000040"),
    );

    let p = iw
        .cv_params
        .iter()
        .find(|p| p.accession.as_deref() == Some("MS:1000829"))
        .expect("missing MS:1000829 (isolation window upper offset)");
    assert_cv_param(
        p,
        Some("MS"),
        Some("MS:1000829"),
        "isolation window upper offset",
        Some("485"),
        Some("MS"),
        Some("m/z"),
        Some("MS:1000040"),
    );

    let sil = precursor
        .selected_ion_list
        .as_ref()
        .expect("missing selectedIonList");
    assert_eq!(sil.count, Some(1));
    assert_eq!(sil.selected_ions.len(), 1);

    let si = &sil.selected_ions[0];

    let p = si
        .cv_params
        .iter()
        .find(|p| p.accession.as_deref() == Some("MS:1000744"))
        .expect("missing MS:1000744 (selected ion m/z)");
    assert_cv_param(
        p,
        Some("MS"),
        Some("MS:1000744"),
        "selected ion m/z",
        Some("515"),
        Some("MS"),
        Some("m/z"),
        Some("MS:1000040"),
    );

    let act = precursor.activation.as_ref().expect("missing activation");

    let p = act
        .cv_params
        .iter()
        .find(|p| p.accession.as_deref() == Some("MS:1001880"))
        .expect("missing MS:1001880 (in-source collision-induced dissociation)");
    assert_cv_param(
        p,
        Some("MS"),
        Some("MS:1001880"),
        "in-source collision-induced dissociation",
        None,
        None,
        None,
        None,
    );

    let p = act
        .cv_params
        .iter()
        .find(|p| p.accession.as_deref() == Some("MS:1000045"))
        .expect("missing MS:1000045 (collision energy)");
    assert_cv_param(
        p,
        Some("MS"),
        Some("MS:1000045"),
        "collision energy",
        Some("20"),
        None,
        None,
        None,
    );
}
