mod common;

use common::helpers::{ensure_first_product_mut, ensure_referenceable_param_group};
use common::test_files;
use common::{
    chromatogram_by_id, decode_ion, encode_to_ion, first_scan, first_scan_mut,
    product_list_of_spectrum, spectrum_by_id,
};
use ionic::mzml::structs::*;

#[test]
fn scan_attributes_not_lost() {
    let mut src = test_files::tiny_pwiz_11().clone();
    let source_file_id = src
        .file_description
        .as_ref()
        .expect("fileDescription")
        .source_file_list
        .source_file
        .first()
        .expect("sourceFile")
        .id
        .clone();
    let instrument_id = src
        .instrument_list
        .as_ref()
        .and_then(|il| il.instrument.first())
        .expect("instrument")
        .id
        .clone();
    let spectrum_ref_id = src
        .run
        .spectrum_list
        .as_ref()
        .and_then(|sl| sl.spectra.get(1))
        .expect("second spectrum")
        .id
        .clone();

    let first_spectrum = src
        .run
        .spectrum_list
        .as_mut()
        .expect("spectrumList")
        .spectra
        .first_mut()
        .expect("first spectrum");
    let target_spectrum_id = first_spectrum.id.clone();
    let scan = first_scan_mut(first_spectrum);
    scan.instrument_configuration_ref = Some(instrument_id.clone());
    scan.source_file_ref = Some(source_file_id.clone());
    scan.external_spectrum_id = Some("external-scan-id:42".to_string());
    scan.spectrum_ref = Some(spectrum_ref_id.clone());

    let out = decode_ion(&encode_to_ion(&src, 12, false)).expect("decode should succeed");
    let out_scan = first_scan(spectrum_by_id(&out, &target_spectrum_id));
    assert_eq!(
        out_scan.instrument_configuration_ref.as_deref(),
        Some(instrument_id.as_str())
    );
    assert_eq!(
        out_scan.source_file_ref.as_deref(),
        Some(source_file_id.as_str())
    );
    assert_eq!(
        out_scan.external_spectrum_id.as_deref(),
        Some("external-scan-id:42")
    );
    assert_eq!(
        out_scan.spectrum_ref.as_deref(),
        Some(spectrum_ref_id.as_str())
    );
}

#[test]
fn spectrum_product_attributes_not_lost() {
    let mut src = test_files::tiny_pwiz_11().clone();
    let source_file_id = src
        .file_description
        .as_ref()
        .expect("fileDescription")
        .source_file_list
        .source_file
        .first()
        .expect("sourceFile")
        .id
        .clone();
    let spectrum_ref_id = src
        .run
        .spectrum_list
        .as_ref()
        .and_then(|sl| sl.spectra.get(1))
        .expect("second spectrum")
        .id
        .clone();

    let first_spectrum = src
        .run
        .spectrum_list
        .as_mut()
        .expect("spectrumList")
        .spectra
        .first_mut()
        .expect("first spectrum");
    let target_spectrum_id = first_spectrum.id.clone();
    let product = ensure_first_product_mut(first_spectrum);
    product.spectrum_ref = Some(spectrum_ref_id.clone());
    product.source_file_ref = Some(source_file_id.clone());
    product.external_spectrum_id = Some("external-product-id:7".to_string());

    let out = decode_ion(&encode_to_ion(&src, 12, false)).expect("decode should succeed");
    let out_product = product_list_of_spectrum(spectrum_by_id(&out, &target_spectrum_id))
        .and_then(|pl| pl.products.first())
        .expect("spectrum product was dropped in ion roundtrip");
    assert_eq!(
        out_product.spectrum_ref.as_deref(),
        Some(spectrum_ref_id.as_str())
    );
    assert_eq!(
        out_product.source_file_ref.as_deref(),
        Some(source_file_id.as_str())
    );
    assert_eq!(
        out_product.external_spectrum_id.as_deref(),
        Some("external-product-id:7")
    );
}

#[test]
fn chrom_product_attributes_not_lost() {
    let mut src = test_files::tiny_pwiz_11().clone();
    let source_file_id = src
        .file_description
        .as_ref()
        .expect("fileDescription")
        .source_file_list
        .source_file
        .first()
        .expect("sourceFile")
        .id
        .clone();
    let spectrum_ref_id = src
        .run
        .spectrum_list
        .as_ref()
        .and_then(|sl| sl.spectra.first())
        .expect("first spectrum")
        .id
        .clone();

    let first_chromatogram = src
        .run
        .chromatogram_list
        .as_mut()
        .expect("chromatogramList")
        .chromatograms
        .first_mut()
        .expect("first chromatogram");
    let target_chromatogram_id = first_chromatogram.id.clone();
    let product = first_chromatogram
        .product
        .get_or_insert_with(Product::default);
    product.spectrum_ref = Some(spectrum_ref_id.clone());
    product.source_file_ref = Some(source_file_id.clone());
    product.external_spectrum_id = Some("external-chrom-product-id:3".to_string());

    let out = decode_ion(&encode_to_ion(&src, 12, false)).expect("decode should succeed");
    let out_product = chromatogram_by_id(&out, &target_chromatogram_id)
        .product
        .as_ref()
        .expect("chromatogram product was dropped");
    assert_eq!(
        out_product.spectrum_ref.as_deref(),
        Some(spectrum_ref_id.as_str())
    );
    assert_eq!(
        out_product.source_file_ref.as_deref(),
        Some(source_file_id.as_str())
    );
    assert_eq!(
        out_product.external_spectrum_id.as_deref(),
        Some("external-chrom-product-id:3")
    );
}

#[test]
fn scan_referenceable_param_group_refs_not_lost() {
    let mut src = test_files::tiny_pwiz_11().clone();
    let ref_group_id = "pwiz-breaker-scan-ref-group";
    ensure_referenceable_param_group(&mut src, ref_group_id);

    let first_spectrum = src
        .run
        .spectrum_list
        .as_mut()
        .expect("spectrumList")
        .spectra
        .first_mut()
        .expect("first spectrum");
    let target_spectrum_id = first_spectrum.id.clone();
    first_scan_mut(first_spectrum).referenceable_param_group_refs =
        vec![ReferenceableParamGroupRef {
            r#ref: ref_group_id.to_string(),
        }];

    let out = decode_ion(&encode_to_ion(&src, 12, false)).expect("decode should succeed");
    let out_refs =
        &first_scan(spectrum_by_id(&out, &target_spectrum_id)).referenceable_param_group_refs;
    assert!(
        out_refs.iter().any(|r| r.r#ref == ref_group_id),
        "scan rpgRef lost in ion roundtrip"
    );
}

#[test]
fn binary_data_array_refs_not_lost() {
    let mut src = test_files::tiny_pwiz_11().clone();
    let ref_group_id = "pwiz-breaker-bda-ref-group";
    ensure_referenceable_param_group(&mut src, ref_group_id);

    let first_spectrum = src
        .run
        .spectrum_list
        .as_mut()
        .expect("spectrumList")
        .spectra
        .first_mut()
        .expect("first spectrum");
    let target_spectrum_id = first_spectrum.id.clone();
    let first_array = first_spectrum
        .binary_data_array_list
        .as_mut()
        .and_then(|bal| bal.binary_data_arrays.first_mut())
        .expect("first spectrum binaryDataArray");
    first_array.referenceable_param_group_refs = vec![ReferenceableParamGroupRef {
        r#ref: ref_group_id.to_string(),
    }];

    let out = decode_ion(&encode_to_ion(&src, 12, false)).expect("decode should succeed");
    let out_first_array = spectrum_by_id(&out, &target_spectrum_id)
        .binary_data_array_list
        .as_ref()
        .and_then(|bal| bal.binary_data_arrays.first())
        .expect("first spectrum binaryDataArray");
    assert!(
        out_first_array
            .referenceable_param_group_refs
            .iter()
            .any(|r| r.r#ref == ref_group_id),
        "binaryDataArray rpgRef lost in ion roundtrip"
    );
}
