#![allow(dead_code)]

use std::collections::{BTreeSet, HashMap};

use ionic::mzml::structs::*;

use super::{
    bda_role, binary_ext::BinaryDataExt, chromatograms, precursor_list_of_spectrum,
    product_list_of_spectrum, scan_list_of_spectrum, set_of_ids, spectra, top_level_dp_ids,
    top_level_instrument_ids, top_level_sample_ids, top_level_software_ids,
    top_level_source_file_ids,
};

pub const EPS_REL_F64: f64 = 1e-9;
pub const EPS_REL_F32: f64 = 1e-5;

pub struct SemanticContext<'a> {
    ref_groups: HashMap<&'a str, &'a ReferenceableParamGroup>,
}

impl<'a> SemanticContext<'a> {
    pub(crate) fn new(mzml: &'a MzML) -> Self {
        let mut ref_groups = HashMap::new();
        if let Some(list) = mzml.referenceable_param_group_list.as_ref() {
            for group in &list.referenceable_param_groups {
                ref_groups.insert(group.id.as_str(), group);
            }
        }
        Self { ref_groups }
    }

    pub(crate) fn effective_param_signatures(
        &self,
        refs: &[ReferenceableParamGroupRef],
        cv_params: &[CvParam],
        user_params: &[UserParam],
    ) -> Vec<String> {
        let mut out = Vec::with_capacity(cv_params.len() + user_params.len() + refs.len());
        for group_ref in refs {
            if let Some(group) = self.ref_groups.get(group_ref.r#ref.as_str()) {
                for cv_param in &group.cv_params {
                    out.push(cv_param_signature(cv_param));
                }
                for user_param in &group.user_params {
                    out.push(user_param_signature(user_param));
                }
            } else {
                out.push(format!("missing-ref-group:{}", group_ref.r#ref));
            }
        }
        for cv_param in cv_params {
            out.push(cv_param_signature(cv_param));
        }
        for user_param in user_params {
            out.push(user_param_signature(user_param));
        }
        out.sort();
        out
    }
}

pub(crate) fn normalized_text(value: Option<&str>) -> Option<&str> {
    value.and_then(|text| {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    })
}

pub(crate) fn normalized_owned_text(value: Option<&str>) -> String {
    normalized_text(value).unwrap_or("").to_string()
}

pub(crate) fn canonical_version_text(value: Option<&str>) -> Option<String> {
    normalized_text(value).map(|text| {
        if let Ok(number) = text.parse::<f64>()
            && number.fract() == 0.0
        {
            return format!("{number:.0}");
        }
        text.to_string()
    })
}

pub(crate) fn should_treat_as_numeric_text(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return false;
    }

    let unsigned = trimmed.strip_prefix(['+', '-']).unwrap_or(trimmed);

    if unsigned.chars().all(|ch| ch.is_ascii_digit()) {
        return unsigned.len() <= 15 && trimmed.parse::<f64>().is_ok();
    }

    if unsigned.contains(['.', 'e', 'E']) {
        return trimmed.parse::<f64>().is_ok();
    }

    false
}

pub(crate) fn canonical_value_text(value: Option<&str>) -> Option<String> {
    normalized_text(value).map(|text| {
        if should_treat_as_numeric_text(text) {
            let number = text.parse::<f64>().expect("numeric text must parse");
            let mut formatted = format!("{number:.12}");
            while formatted.contains('.') && formatted.ends_with('0') {
                formatted.pop();
            }
            if formatted.ends_with('.') {
                formatted.pop();
            }
            return formatted;
        }
        text.to_string()
    })
}

pub(crate) fn cv_param_signature(param: &CvParam) -> String {
    let canonical_name = if normalized_text(param.accession.as_deref()).is_some() {
        None
    } else {
        normalized_text(Some(param.name.as_str()))
    };
    let canonical_unit_name = if normalized_text(param.unit_accession.as_deref()).is_some() {
        None
    } else {
        normalized_text(param.unit_name.as_deref())
    };
    let canonical_unit_cv_ref = if normalized_text(param.unit_accession.as_deref()).is_some() {
        None
    } else {
        normalized_text(param.unit_cv_ref.as_deref())
    };
    format!(
        "cv|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}",
        normalized_text(param.cv_ref.as_deref()),
        normalized_text(param.accession.as_deref()),
        canonical_name,
        canonical_value_text(param.value.as_deref()),
        canonical_unit_cv_ref,
        canonical_unit_name,
        normalized_text(param.unit_accession.as_deref())
    )
}

pub(crate) fn user_param_signature(param: &UserParam) -> String {
    format!(
        "user|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}",
        normalized_text(Some(param.name.as_str())),
        normalized_text(param.r#type.as_deref()),
        normalized_text(param.unit_accession.as_deref()),
        normalized_text(param.unit_cv_ref.as_deref()),
        normalized_text(param.unit_name.as_deref()),
        normalized_text(param.value.as_deref())
    )
}

pub(crate) fn sorted_signatures(values: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut out: Vec<String> = values.into_iter().collect();
    out.sort();
    out
}

pub(crate) fn source_file_refs_signature(list: Option<&SourceFileRefList>) -> Vec<String> {
    list.map(|list| {
        sorted_signatures(
            list.source_file_refs
                .iter()
                .map(|item| normalized_owned_text(Some(item.r#ref.as_str()))),
        )
    })
    .unwrap_or_default()
}

pub(crate) fn software_param_signature(param: &SoftwareParam) -> String {
    format!(
        "{:?}",
        (
            normalized_text(param.cv_ref.as_deref()),
            normalized_text(Some(param.accession.as_str())),
            None::<&str>,
        )
    )
}

pub(crate) fn source_file_signature(
    context: &SemanticContext<'_>,
    source_file: &SourceFile,
) -> String {
    format!(
        "{:?}",
        (
            normalized_text(Some(source_file.id.as_str())),
            normalized_text(Some(source_file.name.as_str())),
            normalized_text(Some(source_file.location.as_str())),
            context.effective_param_signatures(
                &source_file.referenceable_param_group_ref,
                &source_file.cv_param,
                &source_file.user_param,
            ),
        )
    )
}

pub(crate) fn contact_signature(context: &SemanticContext<'_>, contact: &Contact) -> String {
    format!(
        "{:?}",
        context.effective_param_signatures(
            &contact.referenceable_param_group_refs,
            &contact.cv_params,
            &contact.user_params,
        )
    )
}

pub(crate) fn sample_signature(context: &SemanticContext<'_>, sample: &Sample) -> String {
    format!(
        "{:?}",
        (
            normalized_text(Some(sample.id.as_str())),
            normalized_text(Some(sample.name.as_str())),
            context.effective_param_signatures(
                &sample.referenceable_param_group_refs,
                &sample.cv_params,
                &sample.user_params,
            ),
        )
    )
}

pub(crate) fn component_signature(
    context: &SemanticContext<'_>,
    kind: &str,
    order: Option<u32>,
    refs: &[ReferenceableParamGroupRef],
    cv_params: &[CvParam],
    user_params: &[UserParam],
) -> String {
    format!(
        "{:?}",
        (
            kind,
            order,
            context.effective_param_signatures(refs, cv_params, user_params),
        )
    )
}

pub(crate) fn instrument_signature(
    context: &SemanticContext<'_>,
    instrument: &Instrument,
) -> String {
    let mut components = Vec::new();
    if let Some(component_list) = instrument.component_list.as_ref() {
        for source in &component_list.source {
            components.push(component_signature(
                context,
                "source",
                source.order,
                &source.referenceable_param_group_ref,
                &source.cv_param,
                &source.user_param,
            ));
        }
        for analyzer in &component_list.analyzer {
            components.push(component_signature(
                context,
                "analyzer",
                analyzer.order,
                &analyzer.referenceable_param_group_ref,
                &analyzer.cv_param,
                &analyzer.user_param,
            ));
        }
        for detector in &component_list.detector {
            components.push(component_signature(
                context,
                "detector",
                detector.order,
                &detector.referenceable_param_group_ref,
                &detector.cv_param,
                &detector.user_param,
            ));
        }
    }
    components.sort();

    format!(
        "{:?}",
        (
            normalized_text(Some(instrument.id.as_str())),
            normalized_text(
                instrument
                    .scan_settings_ref
                    .as_ref()
                    .map(|value| value.r#ref.as_str())
            ),
            normalized_text(
                instrument
                    .software_ref
                    .as_ref()
                    .map(|value| value.r#ref.as_str())
            ),
            context.effective_param_signatures(
                &instrument.referenceable_param_group_ref,
                &instrument.cv_param,
                &instrument.user_param,
            ),
            components,
        )
    )
}

pub(crate) fn software_signature(software: &Software) -> String {
    let effective_version = canonical_version_text(software.version.as_deref()).or_else(|| {
        software
            .software_param
            .iter()
            .find_map(|param| canonical_version_text(param.version.as_deref()))
    });

    let mut software_params = software
        .software_param
        .iter()
        .map(software_param_signature)
        .collect::<Vec<_>>();
    software_params.sort();

    let mut cv_params = software
        .cv_param
        .iter()
        .map(cv_param_signature)
        .collect::<Vec<_>>();
    cv_params.sort();

    let mut user_params = software
        .user_params
        .iter()
        .map(user_param_signature)
        .collect::<Vec<_>>();
    user_params.sort();

    format!(
        "{:?}",
        (
            normalized_text(Some(software.id.as_str())),
            effective_version,
            software_params,
            cv_params,
            user_params,
        )
    )
}

pub(crate) fn processing_method_signature(
    context: &SemanticContext<'_>,
    method: &ProcessingMethod,
) -> String {
    format!(
        "{:?}",
        (
            method.order,
            normalized_text(method.software_ref.as_deref()),
            context.effective_param_signatures(
                &method.referenceable_param_group_ref,
                &method.cv_param,
                &method.user_param,
            ),
        )
    )
}

pub(crate) fn data_processing_signature(
    context: &SemanticContext<'_>,
    data_processing: &DataProcessing,
) -> String {
    let mut methods = data_processing
        .processing_method
        .iter()
        .map(|item| processing_method_signature(context, item))
        .collect::<Vec<_>>();
    methods.sort();

    format!(
        "{:?}",
        (
            normalized_text(Some(data_processing.id.as_str())),
            normalized_text(data_processing.software_ref.as_deref()),
            methods,
        )
    )
}

pub(crate) fn target_signature(context: &SemanticContext<'_>, target: &Target) -> String {
    format!(
        "{:?}",
        context.effective_param_signatures(
            &target.referenceable_param_group_refs,
            &target.cv_params,
            &target.user_params,
        )
    )
}

pub(crate) fn scan_settings_signature(
    context: &SemanticContext<'_>,
    scan_settings: &ScanSettings,
) -> String {
    let mut targets = scan_settings
        .target_list
        .as_ref()
        .map(|list| {
            list.targets
                .iter()
                .map(|item| target_signature(context, item))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    targets.sort();

    format!(
        "{:?}",
        (
            normalized_text(scan_settings.id.as_deref()),
            normalized_text(scan_settings.instrument_configuration_ref.as_deref()),
            context.effective_param_signatures(
                &scan_settings.referenceable_param_group_refs,
                &scan_settings.cv_params,
                &scan_settings.user_params,
            ),
            source_file_refs_signature(scan_settings.source_file_ref_list.as_ref()),
            targets,
        )
    )
}

pub(crate) fn run_signature(context: &SemanticContext<'_>, run: &Run) -> String {
    format!(
        "{:?}",
        (
            normalized_text(Some(run.id.as_str())),
            normalized_text(run.start_time_stamp.as_deref()),
            normalized_text(run.default_instrument_configuration_ref.as_deref()),
            normalized_text(run.default_source_file_ref.as_deref()),
            normalized_text(run.sample_ref.as_deref()),
            context.effective_param_signatures(
                &run.referenceable_param_group_refs,
                &run.cv_params,
                &run.user_params,
            ),
            source_file_refs_signature(run.source_file_ref_list.as_ref()),
        )
    )
}

pub(crate) fn spectrum_description_params_signature(
    context: &SemanticContext<'_>,
    description: Option<&SpectrumDescription>,
) -> Vec<String> {
    description
        .map(|description| {
            context.effective_param_signatures(
                &description.referenceable_param_group_refs,
                &description.cv_params,
                &description.user_params,
            )
        })
        .unwrap_or_default()
}

pub(crate) fn assert_signature_vec_eq(left: Vec<String>, right: Vec<String>, context: &str) {
    assert_eq!(left, right, "{context}: semantic signature mismatch");
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn assert_effective_params_eq(
    left_context: &SemanticContext<'_>,
    left_refs: &[ReferenceableParamGroupRef],
    left_cv: &[CvParam],
    left_user: &[UserParam],
    right_context: &SemanticContext<'_>,
    right_refs: &[ReferenceableParamGroupRef],
    right_cv: &[CvParam],
    right_user: &[UserParam],
    context: &str,
) {
    let left = left_context.effective_param_signatures(left_refs, left_cv, left_user);
    let right = right_context.effective_param_signatures(right_refs, right_cv, right_user);
    assert_signature_vec_eq(left, right, context);
}

pub(crate) fn assert_opt_str_eq(left: Option<&str>, right: Option<&str>, context: &str) {
    assert_eq!(normalized_text(left), normalized_text(right), "{context}");
}

pub(crate) fn assert_optional_count_eq(label: &str, declared: Option<usize>, actual: usize) {
    if let Some(declared) = declared {
        assert_eq!(declared, actual, "{label}: declared count mismatch");
    }
}

pub(crate) fn assert_optional_count_eq_u32(label: &str, declared: Option<u32>, actual: usize) {
    if let Some(declared) = declared {
        assert_eq!(
            declared as usize, actual,
            "{label}: declared count mismatch"
        );
    }
}

pub(crate) fn rel_close_f64(a: f64, b: f64, eps_rel: f64, context: &str) {
    let diff = (a - b).abs();
    let scale = a.abs().max(b.abs()).max(1.0);
    assert!(
        diff <= scale * eps_rel,
        "{context}: values differ: left={a} right={b} diff={diff} allowed={} (rel={eps_rel})",
        scale * eps_rel
    );
}

pub(crate) fn assert_binary_semantic_eq(left: &NumericArray, right: &NumericArray, context: &str) {
    match (left, right) {
        (NumericArray::F64(l), NumericArray::F64(r)) => {
            assert_eq!(l.len(), r.len(), "{context}: f64 len mismatch");
            for (i, (lv, rv)) in l.iter().zip(r.iter()).enumerate() {
                rel_close_f64(*lv, *rv, EPS_REL_F64, &format!("{context} f64[{i}]"));
            }
        }
        (NumericArray::F32(l), NumericArray::F32(r)) => {
            assert_eq!(l.len(), r.len(), "{context}: f32 len mismatch");
            for (i, (lv, rv)) in l.iter().zip(r.iter()).enumerate() {
                rel_close_f64(
                    *lv as f64,
                    *rv as f64,
                    EPS_REL_F32,
                    &format!("{context} f32[{i}]"),
                );
            }
        }
        (NumericArray::F16(l), NumericArray::F16(r)) => {
            assert_eq!(l, r, "{context}: f16 payload mismatch")
        }
        (NumericArray::I64(l), NumericArray::I64(r)) => {
            assert_eq!(l, r, "{context}: i64 payload mismatch")
        }
        (NumericArray::I32(l), NumericArray::I32(r)) => {
            assert_eq!(l, r, "{context}: i32 payload mismatch")
        }
        (NumericArray::I16(l), NumericArray::I16(r)) => {
            assert_eq!(l, r, "{context}: i16 payload mismatch")
        }
        (l, r) => panic!("{context}: binary variant mismatch: left={l:?} right={r:?}"),
    }
}

pub(crate) fn binary_semantically_empty(binary: Option<&NumericArray>) -> bool {
    match binary {
        None => true,
        Some(binary) => binary.is_empty(),
    }
}

pub(crate) fn effective_data_processing_ref<'a>(
    raw: Option<&'a str>,
    default_ref: Option<&'a str>,
) -> Option<&'a str> {
    normalized_text(raw).or_else(|| normalized_text(default_ref))
}

pub(crate) fn effective_binary_data_array_length(array: &BinaryDataArray) -> Option<usize> {
    array
        .array_length
        .or_else(|| array.binary.as_ref().map(|b| b.len()))
}

pub(crate) fn assert_binary_data_array_semantic_eq(
    left_context: &SemanticContext<'_>,
    right_context: &SemanticContext<'_>,
    left: &BinaryDataArray,
    right: &BinaryDataArray,
    context: &str,
) {
    assert_eq!(
        effective_binary_data_array_length(left),
        effective_binary_data_array_length(right),
        "{context}: arrayLength mismatch"
    );
    assert_opt_str_eq(
        left.data_processing_ref.as_deref(),
        right.data_processing_ref.as_deref(),
        &format!("{context}: dataProcessingRef mismatch"),
    );
    assert_eq!(
        left.numeric_type, right.numeric_type,
        "{context}: numeric_type mismatch"
    );
    assert_effective_params_eq(
        left_context,
        &left.referenceable_param_group_refs,
        &left.cv_params,
        &left.user_params,
        right_context,
        &right.referenceable_param_group_refs,
        &right.cv_params,
        &right.user_params,
        &format!("{context}: parameter bundle mismatch"),
    );

    match (left.binary.as_ref(), right.binary.as_ref()) {
        (Some(left_binary), Some(right_binary)) => {
            assert_eq!(
                left_binary.variant_name(),
                right_binary.variant_name(),
                "{context}: binary variant mismatch"
            );
            assert_binary_semantic_eq(left_binary, right_binary, context);
        }
        _ => {
            assert!(
                binary_semantically_empty(left.binary.as_ref())
                    && binary_semantically_empty(right.binary.as_ref()),
                "{context}: one side has semantic payload while the other is empty"
            );
        }
    }
}

pub(crate) fn assert_binary_data_array_list_semantic_eq(
    left_context: &SemanticContext<'_>,
    right_context: &SemanticContext<'_>,
    left: Option<&BinaryDataArrayList>,
    right: Option<&BinaryDataArrayList>,
    context: &str,
) {
    match (left, right) {
        (None, None) => {}
        (Some(left), Some(right)) => {
            assert_eq!(
                left.binary_data_arrays.len(),
                right.binary_data_arrays.len(),
                "{context}: binaryDataArray count mismatch"
            );
            for (index, (left, right)) in left
                .binary_data_arrays
                .iter()
                .zip(&right.binary_data_arrays)
                .enumerate()
            {
                let array_context = format!("{context} array[{index}] role={}", bda_role(left));
                assert_eq!(
                    bda_role(left),
                    bda_role(right),
                    "{array_context}: role mismatch"
                );
                assert_binary_data_array_semantic_eq(
                    left_context,
                    right_context,
                    left,
                    right,
                    &array_context,
                );
            }
        }
        _ => panic!("{context}: binaryDataArrayList presence mismatch"),
    }
}

pub(crate) fn assert_scan_window_list_semantic_eq(
    left_context: &SemanticContext<'_>,
    right_context: &SemanticContext<'_>,
    left: Option<&ScanWindowList>,
    right: Option<&ScanWindowList>,
    context: &str,
) {
    match (left, right) {
        (None, None) => {}
        (Some(left), Some(right)) => {
            assert_eq!(
                left.scan_windows.len(),
                right.scan_windows.len(),
                "{context}: scanWindow count mismatch"
            );
            for (index, (left_window, right_window)) in left
                .scan_windows
                .iter()
                .zip(&right.scan_windows)
                .enumerate()
            {
                assert_effective_params_eq(
                    left_context,
                    &[],
                    &left_window.cv_params,
                    &left_window.user_params,
                    right_context,
                    &[],
                    &right_window.cv_params,
                    &right_window.user_params,
                    &format!("{context}: scanWindow[{index}] params mismatch"),
                );
            }
        }
        _ => panic!("{context}: scanWindowList presence mismatch"),
    }
}

pub(crate) fn assert_scan_semantic_eq(
    left_context: &SemanticContext<'_>,
    right_context: &SemanticContext<'_>,
    left: &Scan,
    right: &Scan,
    context: &str,
) {
    assert_opt_str_eq(
        left.instrument_configuration_ref.as_deref(),
        right.instrument_configuration_ref.as_deref(),
        &format!("{context}: instrumentConfigurationRef mismatch"),
    );
    assert_opt_str_eq(
        left.external_spectrum_id.as_deref(),
        right.external_spectrum_id.as_deref(),
        &format!("{context}: externalSpectrumID mismatch"),
    );
    assert_opt_str_eq(
        left.source_file_ref.as_deref(),
        right.source_file_ref.as_deref(),
        &format!("{context}: sourceFileRef mismatch"),
    );
    assert_opt_str_eq(
        left.spectrum_ref.as_deref(),
        right.spectrum_ref.as_deref(),
        &format!("{context}: spectrumRef mismatch"),
    );
    assert_effective_params_eq(
        left_context,
        &left.referenceable_param_group_refs,
        &left.cv_params,
        &left.user_params,
        right_context,
        &right.referenceable_param_group_refs,
        &right.cv_params,
        &right.user_params,
        &format!("{context}: parameter bundle mismatch"),
    );
    assert_scan_window_list_semantic_eq(
        left_context,
        right_context,
        left.scan_window_list.as_ref(),
        right.scan_window_list.as_ref(),
        &format!("{context}: scanWindowList"),
    );
}

pub(crate) fn assert_scan_list_semantic_eq(
    left_context: &SemanticContext<'_>,
    right_context: &SemanticContext<'_>,
    left: Option<&ScanList>,
    right: Option<&ScanList>,
    context: &str,
) {
    match (left, right) {
        (None, None) => {}
        (Some(left), Some(right)) => {
            assert_effective_params_eq(
                left_context,
                &[],
                &left.cv_params,
                &left.user_params,
                right_context,
                &[],
                &right.cv_params,
                &right.user_params,
                &format!("{context}: scanList params mismatch"),
            );
            assert_eq!(
                left.scans.len(),
                right.scans.len(),
                "{context}: scan count mismatch"
            );
            for (index, (left_scan, right_scan)) in left.scans.iter().zip(&right.scans).enumerate()
            {
                assert_scan_semantic_eq(
                    left_context,
                    right_context,
                    left_scan,
                    right_scan,
                    &format!("{context}: scan[{index}]"),
                );
            }
        }
        _ => panic!("{context}: scanList presence mismatch"),
    }
}

pub(crate) fn assert_isolation_window_semantic_eq(
    left_context: &SemanticContext<'_>,
    right_context: &SemanticContext<'_>,
    left: Option<&IsolationWindow>,
    right: Option<&IsolationWindow>,
    context: &str,
) {
    match (left, right) {
        (None, None) => {}
        (Some(left), Some(right)) => {
            assert_effective_params_eq(
                left_context,
                &left.referenceable_param_group_refs,
                &left.cv_params,
                &left.user_params,
                right_context,
                &right.referenceable_param_group_refs,
                &right.cv_params,
                &right.user_params,
                context,
            );
        }
        _ => panic!("{context}: isolationWindow presence mismatch"),
    }
}

pub(crate) fn assert_selected_ion_list_semantic_eq(
    left_context: &SemanticContext<'_>,
    right_context: &SemanticContext<'_>,
    left: Option<&SelectedIonList>,
    right: Option<&SelectedIonList>,
    context: &str,
) {
    match (left, right) {
        (None, None) => {}
        (Some(left), Some(right)) => {
            assert_eq!(
                left.selected_ions.len(),
                right.selected_ions.len(),
                "{context}: selectedIon count mismatch"
            );
            for (index, (left_ion, right_ion)) in left
                .selected_ions
                .iter()
                .zip(&right.selected_ions)
                .enumerate()
            {
                assert_effective_params_eq(
                    left_context,
                    &left_ion.referenceable_param_group_refs,
                    &left_ion.cv_params,
                    &left_ion.user_params,
                    right_context,
                    &right_ion.referenceable_param_group_refs,
                    &right_ion.cv_params,
                    &right_ion.user_params,
                    &format!("{context}: selectedIon[{index}] params mismatch"),
                );
            }
        }
        _ => panic!("{context}: selectedIonList presence mismatch"),
    }
}

pub(crate) fn assert_activation_semantic_eq(
    left_context: &SemanticContext<'_>,
    right_context: &SemanticContext<'_>,
    left: Option<&Activation>,
    right: Option<&Activation>,
    context: &str,
) {
    match (left, right) {
        (None, None) => {}
        (Some(left), Some(right)) => {
            assert_effective_params_eq(
                left_context,
                &left.referenceable_param_group_refs,
                &left.cv_params,
                &left.user_params,
                right_context,
                &right.referenceable_param_group_refs,
                &right.cv_params,
                &right.user_params,
                context,
            );
        }
        _ => panic!("{context}: activation presence mismatch"),
    }
}

pub(crate) fn assert_precursor_semantic_eq(
    left_context: &SemanticContext<'_>,
    right_context: &SemanticContext<'_>,
    left: &Precursor,
    right: &Precursor,
    context: &str,
) {
    assert_opt_str_eq(
        left.spectrum_ref.as_deref(),
        right.spectrum_ref.as_deref(),
        &format!("{context}: spectrumRef mismatch"),
    );
    assert_opt_str_eq(
        left.source_file_ref.as_deref(),
        right.source_file_ref.as_deref(),
        &format!("{context}: sourceFileRef mismatch"),
    );
    assert_opt_str_eq(
        left.external_spectrum_id.as_deref(),
        right.external_spectrum_id.as_deref(),
        &format!("{context}: externalSpectrumID mismatch"),
    );
    assert_isolation_window_semantic_eq(
        left_context,
        right_context,
        left.isolation_window.as_ref(),
        right.isolation_window.as_ref(),
        &format!("{context}: isolationWindow"),
    );
    assert_selected_ion_list_semantic_eq(
        left_context,
        right_context,
        left.selected_ion_list.as_ref(),
        right.selected_ion_list.as_ref(),
        &format!("{context}: selectedIonList"),
    );
    assert_activation_semantic_eq(
        left_context,
        right_context,
        left.activation.as_ref(),
        right.activation.as_ref(),
        &format!("{context}: activation"),
    );
}

pub(crate) fn assert_precursor_list_semantic_eq(
    left_context: &SemanticContext<'_>,
    right_context: &SemanticContext<'_>,
    left: Option<&PrecursorList>,
    right: Option<&PrecursorList>,
    context: &str,
) {
    match (left, right) {
        (None, None) => {}
        (Some(left), Some(right)) => {
            assert_effective_params_eq(
                left_context,
                &[],
                &left.cv_params,
                &left.user_params,
                right_context,
                &[],
                &right.cv_params,
                &right.user_params,
                &format!("{context}: precursorList params mismatch"),
            );
            assert_eq!(
                left.precursors.len(),
                right.precursors.len(),
                "{context}: precursor count mismatch"
            );
            for (index, (left_precursor, right_precursor)) in
                left.precursors.iter().zip(&right.precursors).enumerate()
            {
                assert_precursor_semantic_eq(
                    left_context,
                    right_context,
                    left_precursor,
                    right_precursor,
                    &format!("{context}: precursor[{index}]"),
                );
            }
        }
        _ => panic!("{context}: precursorList presence mismatch"),
    }
}

pub(crate) fn assert_product_semantic_eq(
    left_context: &SemanticContext<'_>,
    right_context: &SemanticContext<'_>,
    left: &Product,
    right: &Product,
    context: &str,
) {
    assert_opt_str_eq(
        left.spectrum_ref.as_deref(),
        right.spectrum_ref.as_deref(),
        &format!("{context}: spectrumRef mismatch"),
    );
    assert_opt_str_eq(
        left.source_file_ref.as_deref(),
        right.source_file_ref.as_deref(),
        &format!("{context}: sourceFileRef mismatch"),
    );
    assert_opt_str_eq(
        left.external_spectrum_id.as_deref(),
        right.external_spectrum_id.as_deref(),
        &format!("{context}: externalSpectrumID mismatch"),
    );
    assert_isolation_window_semantic_eq(
        left_context,
        right_context,
        left.isolation_window.as_ref(),
        right.isolation_window.as_ref(),
        &format!("{context}: isolationWindow"),
    );
    assert_effective_params_eq(
        left_context,
        &[],
        &left.cv_params,
        &left.user_params,
        right_context,
        &[],
        &right.cv_params,
        &right.user_params,
        &format!("{context}: parameter bundle mismatch"),
    );
}

pub(crate) fn assert_product_list_semantic_eq(
    left_context: &SemanticContext<'_>,
    right_context: &SemanticContext<'_>,
    left: Option<&ProductList>,
    right: Option<&ProductList>,
    context: &str,
) {
    match (left, right) {
        (None, None) => {}
        (Some(left), Some(right)) => {
            assert_effective_params_eq(
                left_context,
                &[],
                &left.cv_params,
                &left.user_params,
                right_context,
                &[],
                &right.cv_params,
                &right.user_params,
                &format!("{context}: productList params mismatch"),
            );
            assert_eq!(
                left.products.len(),
                right.products.len(),
                "{context}: product count mismatch"
            );
            for (index, (left_product, right_product)) in
                left.products.iter().zip(&right.products).enumerate()
            {
                assert_product_semantic_eq(
                    left_context,
                    right_context,
                    left_product,
                    right_product,
                    &format!("{context}: product[{index}]"),
                );
            }
        }
        _ => panic!("{context}: productList presence mismatch"),
    }
}

pub(crate) fn assert_spectrum_semantic_eq(
    left_context: &SemanticContext<'_>,
    right_context: &SemanticContext<'_>,
    left: &Spectrum,
    right: &Spectrum,
    left_default_data_processing_ref: Option<&str>,
    right_default_data_processing_ref: Option<&str>,
    context: &str,
) {
    assert_eq!(left.id, right.id, "{context}: spectrum id mismatch");
    assert_eq!(
        left.index, right.index,
        "{context}: spectrum index mismatch"
    );
    assert_eq!(
        left.scan_number, right.scan_number,
        "{context}: scan number mismatch"
    );
    assert_eq!(
        left.default_array_length, right.default_array_length,
        "{context}: defaultArrayLength mismatch"
    );
    assert_opt_str_eq(
        left.native_id.as_deref(),
        right.native_id.as_deref(),
        &format!("{context}: nativeID mismatch"),
    );
    assert_opt_str_eq(
        effective_data_processing_ref(
            left.data_processing_ref.as_deref(),
            left_default_data_processing_ref,
        ),
        effective_data_processing_ref(
            right.data_processing_ref.as_deref(),
            right_default_data_processing_ref,
        ),
        &format!("{context}: dataProcessingRef mismatch"),
    );
    assert_opt_str_eq(
        left.source_file_ref.as_deref(),
        right.source_file_ref.as_deref(),
        &format!("{context}: sourceFileRef mismatch"),
    );
    assert_opt_str_eq(
        left.spot_id.as_deref(),
        right.spot_id.as_deref(),
        &format!("{context}: spotID mismatch"),
    );
    if let (Some(l), Some(r)) = (left.ms_level, right.ms_level) {
        assert_eq!(l, r, "{context}: msLevel mismatch");
    }

    assert_effective_params_eq(
        left_context,
        &left.referenceable_param_group_refs,
        &left.cv_params,
        &left.user_params,
        right_context,
        &right.referenceable_param_group_refs,
        &right.cv_params,
        &right.user_params,
        &format!("{context}: spectrum parameter bundle mismatch"),
    );
    assert_signature_vec_eq(
        spectrum_description_params_signature(left_context, left.spectrum_description.as_ref()),
        spectrum_description_params_signature(right_context, right.spectrum_description.as_ref()),
        &format!("{context}: spectrumDescription parameter bundle mismatch"),
    );

    assert_scan_list_semantic_eq(
        left_context,
        right_context,
        scan_list_of_spectrum(left),
        scan_list_of_spectrum(right),
        &format!("{context}: scanList"),
    );
    assert_precursor_list_semantic_eq(
        left_context,
        right_context,
        precursor_list_of_spectrum(left),
        precursor_list_of_spectrum(right),
        &format!("{context}: precursorList"),
    );
    assert_product_list_semantic_eq(
        left_context,
        right_context,
        product_list_of_spectrum(left),
        product_list_of_spectrum(right),
        &format!("{context}: productList"),
    );
    assert_binary_data_array_list_semantic_eq(
        left_context,
        right_context,
        left.binary_data_array_list.as_ref(),
        right.binary_data_array_list.as_ref(),
        &format!("{context}: binaryDataArrayList"),
    );
}

pub(crate) fn assert_chromatogram_semantic_eq(
    left_context: &SemanticContext<'_>,
    right_context: &SemanticContext<'_>,
    left: &Chromatogram,
    right: &Chromatogram,
    left_default_data_processing_ref: Option<&str>,
    right_default_data_processing_ref: Option<&str>,
    context: &str,
) {
    assert_eq!(left.id, right.id, "{context}: chromatogram id mismatch");
    assert_opt_str_eq(
        left.native_id.as_deref(),
        right.native_id.as_deref(),
        &format!("{context}: chromatogram nativeID mismatch"),
    );
    assert_eq!(
        left.index, right.index,
        "{context}: chromatogram index mismatch"
    );
    assert_eq!(
        left.default_array_length, right.default_array_length,
        "{context}: defaultArrayLength mismatch"
    );
    assert_opt_str_eq(
        effective_data_processing_ref(
            left.data_processing_ref.as_deref(),
            left_default_data_processing_ref,
        ),
        effective_data_processing_ref(
            right.data_processing_ref.as_deref(),
            right_default_data_processing_ref,
        ),
        &format!("{context}: chromatogram dataProcessingRef mismatch"),
    );
    assert_effective_params_eq(
        left_context,
        &left.referenceable_param_group_refs,
        &left.cv_params,
        &left.user_params,
        right_context,
        &right.referenceable_param_group_refs,
        &right.cv_params,
        &right.user_params,
        &format!("{context}: chromatogram parameter bundle mismatch"),
    );
    match (left.precursor.as_ref(), right.precursor.as_ref()) {
        (None, None) => {}
        (Some(left_precursor), Some(right_precursor)) => assert_precursor_semantic_eq(
            left_context,
            right_context,
            left_precursor,
            right_precursor,
            &format!("{context}: chromatogram precursor"),
        ),
        _ => panic!("{context}: chromatogram precursor presence mismatch"),
    }
    match (left.product.as_ref(), right.product.as_ref()) {
        (None, None) => {}
        (Some(left_product), Some(right_product)) => assert_product_semantic_eq(
            left_context,
            right_context,
            left_product,
            right_product,
            &format!("{context}: chromatogram product"),
        ),
        _ => panic!("{context}: chromatogram product presence mismatch"),
    }
    assert_binary_data_array_list_semantic_eq(
        left_context,
        right_context,
        left.binary_data_array_list.as_ref(),
        right.binary_data_array_list.as_ref(),
        &format!("{context}: chromatogram binaryDataArrayList"),
    );
}

pub(crate) fn assert_declared_counts_consistent(mzml: &MzML) {
    if let Some(list) = mzml.cv_list.as_ref() {
        assert_optional_count_eq("cvList", list.count, list.cv.len());
    }
    if let Some(list) = mzml.referenceable_param_group_list.as_ref() {
        assert_optional_count_eq(
            "referenceableParamGroupList",
            list.count,
            list.referenceable_param_groups.len(),
        );
    }
    if let Some(file_description) = mzml.file_description.as_ref() {
        assert_optional_count_eq(
            "sourceFileList",
            file_description.source_file_list.count,
            file_description.source_file_list.source_file.len(),
        );
    }
    if let Some(list) = mzml.sample_list.as_ref() {
        assert_optional_count_eq_u32("sampleList", list.count, list.samples.len());
    }
    if let Some(list) = mzml.instrument_list.as_ref() {
        assert_optional_count_eq("instrumentList", list.count, list.instrument.len());
        for (index, instrument) in list.instrument.iter().enumerate() {
            if let Some(component_list) = instrument.component_list.as_ref() {
                let actual = component_list.source.len()
                    + component_list.analyzer.len()
                    + component_list.detector.len();
                assert_optional_count_eq(
                    &format!("instrument[{index}].componentList"),
                    component_list.count,
                    actual,
                );
            }
        }
    }
    if let Some(list) = mzml.software_list.as_ref() {
        assert_optional_count_eq("softwareList", list.count, list.software.len());
    }
    if let Some(list) = mzml.data_processing_list.as_ref() {
        assert_optional_count_eq("dataProcessingList", list.count, list.data_processing.len());
    }
    if let Some(list) = mzml.scan_settings_list.as_ref() {
        assert_optional_count_eq("scanSettingsList", list.count, list.scan_settings.len());
        for (index, scan_settings) in list.scan_settings.iter().enumerate() {
            if let Some(source_file_refs) = scan_settings.source_file_ref_list.as_ref() {
                assert_optional_count_eq(
                    &format!("scanSettings[{index}].sourceFileRefList"),
                    source_file_refs.count,
                    source_file_refs.source_file_refs.len(),
                );
            }
            if let Some(target_list) = scan_settings.target_list.as_ref() {
                assert_optional_count_eq(
                    &format!("scanSettings[{index}].targetList"),
                    target_list.count,
                    target_list.targets.len(),
                );
            }
        }
    }
    if let Some(source_file_refs) = mzml.run.source_file_ref_list.as_ref() {
        assert_optional_count_eq(
            "run.sourceFileRefList",
            source_file_refs.count,
            source_file_refs.source_file_refs.len(),
        );
    }
    if let Some(list) = mzml.run.spectrum_list.as_ref() {
        assert_optional_count_eq("spectrumList", list.count, list.spectra.len());
        for (index, spectrum) in list.spectra.iter().enumerate() {
            if let Some(scan_list) = scan_list_of_spectrum(spectrum) {
                assert_optional_count_eq(
                    &format!("spectrum[{index}].scanList"),
                    scan_list.count,
                    scan_list.scans.len(),
                );
                for (scan_index, scan) in scan_list.scans.iter().enumerate() {
                    if let Some(scan_window_list) = scan.scan_window_list.as_ref() {
                        assert_optional_count_eq(
                            &format!("spectrum[{index}].scan[{scan_index}].scanWindowList"),
                            scan_window_list.count,
                            scan_window_list.scan_windows.len(),
                        );
                    }
                }
            }
            if let Some(precursor_list) = precursor_list_of_spectrum(spectrum) {
                assert_optional_count_eq(
                    &format!("spectrum[{index}].precursorList"),
                    precursor_list.count,
                    precursor_list.precursors.len(),
                );
                for (precursor_index, precursor) in precursor_list.precursors.iter().enumerate() {
                    if let Some(selected_ion_list) = precursor.selected_ion_list.as_ref() {
                        assert_optional_count_eq(
                            &format!(
                                "spectrum[{index}].precursor[{precursor_index}].selectedIonList"
                            ),
                            selected_ion_list.count,
                            selected_ion_list.selected_ions.len(),
                        );
                    }
                }
            }
            if let Some(product_list) = product_list_of_spectrum(spectrum) {
                assert_optional_count_eq(
                    &format!("spectrum[{index}].productList"),
                    product_list.count,
                    product_list.products.len(),
                );
            }
            if let Some(binary_data_array_list) = spectrum.binary_data_array_list.as_ref() {
                assert_optional_count_eq(
                    &format!("spectrum[{index}].binaryDataArrayList"),
                    binary_data_array_list.count,
                    binary_data_array_list.binary_data_arrays.len(),
                );
                assert_binary_data_array_lengths_consistent(
                    &binary_data_array_list.binary_data_arrays,
                    &format!("spectrum[{index}]"),
                );
            }
        }
    }
    if let Some(list) = mzml.run.chromatogram_list.as_ref() {
        assert_optional_count_eq("chromatogramList", list.count, list.chromatograms.len());
        for (index, chromatogram) in list.chromatograms.iter().enumerate() {
            if let Some(binary_data_array_list) = chromatogram.binary_data_array_list.as_ref() {
                assert_optional_count_eq(
                    &format!("chromatogram[{index}].binaryDataArrayList"),
                    binary_data_array_list.count,
                    binary_data_array_list.binary_data_arrays.len(),
                );
                assert_binary_data_array_lengths_consistent(
                    &binary_data_array_list.binary_data_arrays,
                    &format!("chromatogram[{index}]"),
                );
            }
        }
    }
}

pub(crate) fn assert_mzml_semantic_eq(left: &MzML, right: &MzML) {
    assert_all_refs_resolved(left);
    assert_all_refs_resolved(right);

    let left_context = SemanticContext::new(left);
    let right_context = SemanticContext::new(right);

    assert_signature_vec_eq(
        left.cv_list
            .as_ref()
            .map(|list| {
                sorted_signatures(list.cv.iter().map(|entry| {
                    format!(
                        "{:?}",
                        (
                            normalized_text(Some(entry.id.as_str())),
                            normalized_text(entry.full_name.as_deref()),
                            normalized_text(entry.version.as_deref()),
                            normalized_text(entry.uri.as_deref()),
                        )
                    )
                }))
            })
            .unwrap_or_default(),
        right
            .cv_list
            .as_ref()
            .map(|list| {
                sorted_signatures(list.cv.iter().map(|entry| {
                    format!(
                        "{:?}",
                        (
                            normalized_text(Some(entry.id.as_str())),
                            normalized_text(entry.full_name.as_deref()),
                            normalized_text(entry.version.as_deref()),
                            normalized_text(entry.uri.as_deref()),
                        )
                    )
                }))
            })
            .unwrap_or_default(),
        "cvList mismatch",
    );

    match (
        left.file_description.as_ref(),
        right.file_description.as_ref(),
    ) {
        (None, None) => {}
        (Some(left_file_description), Some(right_file_description)) => {
            assert_effective_params_eq(
                &left_context,
                &left_file_description
                    .file_content
                    .referenceable_param_group_refs,
                &left_file_description.file_content.cv_params,
                &left_file_description.file_content.user_params,
                &right_context,
                &right_file_description
                    .file_content
                    .referenceable_param_group_refs,
                &right_file_description.file_content.cv_params,
                &right_file_description.file_content.user_params,
                "fileDescription.fileContent semantic mismatch",
            );
            assert_signature_vec_eq(
                sorted_signatures(
                    left_file_description
                        .source_file_list
                        .source_file
                        .iter()
                        .map(|item| source_file_signature(&left_context, item)),
                ),
                sorted_signatures(
                    right_file_description
                        .source_file_list
                        .source_file
                        .iter()
                        .map(|item| source_file_signature(&right_context, item)),
                ),
                "fileDescription.sourceFileList mismatch",
            );
            assert_signature_vec_eq(
                sorted_signatures(
                    left_file_description
                        .contacts
                        .iter()
                        .map(|item| contact_signature(&left_context, item)),
                ),
                sorted_signatures(
                    right_file_description
                        .contacts
                        .iter()
                        .map(|item| contact_signature(&right_context, item)),
                ),
                "fileDescription.contacts mismatch",
            );
        }
        _ => panic!("fileDescription presence mismatch"),
    }

    assert_signature_vec_eq(
        left.sample_list
            .as_ref()
            .map(|list| {
                sorted_signatures(
                    list.samples
                        .iter()
                        .map(|item| sample_signature(&left_context, item)),
                )
            })
            .unwrap_or_default(),
        right
            .sample_list
            .as_ref()
            .map(|list| {
                sorted_signatures(
                    list.samples
                        .iter()
                        .map(|item| sample_signature(&right_context, item)),
                )
            })
            .unwrap_or_default(),
        "sampleList mismatch",
    );
    assert_signature_vec_eq(
        left.instrument_list
            .as_ref()
            .map(|list| {
                sorted_signatures(
                    list.instrument
                        .iter()
                        .map(|item| instrument_signature(&left_context, item)),
                )
            })
            .unwrap_or_default(),
        right
            .instrument_list
            .as_ref()
            .map(|list| {
                sorted_signatures(
                    list.instrument
                        .iter()
                        .map(|item| instrument_signature(&right_context, item)),
                )
            })
            .unwrap_or_default(),
        "instrumentList mismatch",
    );
    assert_signature_vec_eq(
        left.software_list
            .as_ref()
            .map(|list| sorted_signatures(list.software.iter().map(software_signature)))
            .unwrap_or_default(),
        right
            .software_list
            .as_ref()
            .map(|list| sorted_signatures(list.software.iter().map(software_signature)))
            .unwrap_or_default(),
        "softwareList mismatch",
    );
    assert_signature_vec_eq(
        left.data_processing_list
            .as_ref()
            .map(|list| {
                sorted_signatures(
                    list.data_processing
                        .iter()
                        .map(|item| data_processing_signature(&left_context, item)),
                )
            })
            .unwrap_or_default(),
        right
            .data_processing_list
            .as_ref()
            .map(|list| {
                sorted_signatures(
                    list.data_processing
                        .iter()
                        .map(|item| data_processing_signature(&right_context, item)),
                )
            })
            .unwrap_or_default(),
        "dataProcessingList mismatch",
    );
    assert_signature_vec_eq(
        left.scan_settings_list
            .as_ref()
            .map(|list| {
                sorted_signatures(
                    list.scan_settings
                        .iter()
                        .map(|item| scan_settings_signature(&left_context, item)),
                )
            })
            .unwrap_or_default(),
        right
            .scan_settings_list
            .as_ref()
            .map(|list| {
                sorted_signatures(
                    list.scan_settings
                        .iter()
                        .map(|item| scan_settings_signature(&right_context, item)),
                )
            })
            .unwrap_or_default(),
        "scanSettingsList mismatch",
    );

    assert_eq!(
        run_signature(&left_context, &left.run),
        run_signature(&right_context, &right.run),
        "run metadata mismatch"
    );

    match (
        left.run.spectrum_list.as_ref(),
        right.run.spectrum_list.as_ref(),
    ) {
        (Some(left_spectrum_list), Some(right_spectrum_list)) => {
            assert_eq!(
                left_spectrum_list.spectra.len(),
                right_spectrum_list.spectra.len(),
                "spectrum count mismatch"
            );
            for (index, (left_spectrum, right_spectrum)) in left_spectrum_list
                .spectra
                .iter()
                .zip(&right_spectrum_list.spectra)
                .enumerate()
            {
                assert_spectrum_semantic_eq(
                    &left_context,
                    &right_context,
                    left_spectrum,
                    right_spectrum,
                    left_spectrum_list.default_data_processing_ref.as_deref(),
                    right_spectrum_list.default_data_processing_ref.as_deref(),
                    &format!("spectrum[{index}]"),
                );
            }
        }
        (None, None) => {}
        _ => panic!("spectrumList presence mismatch"),
    }

    match (
        left.run.chromatogram_list.as_ref(),
        right.run.chromatogram_list.as_ref(),
    ) {
        (Some(left_chromatogram_list), Some(right_chromatogram_list)) => {
            assert_eq!(
                left_chromatogram_list.chromatograms.len(),
                right_chromatogram_list.chromatograms.len(),
                "chromatogram count mismatch"
            );
            for (index, (left_chromatogram, right_chromatogram)) in left_chromatogram_list
                .chromatograms
                .iter()
                .zip(&right_chromatogram_list.chromatograms)
                .enumerate()
            {
                assert_chromatogram_semantic_eq(
                    &left_context,
                    &right_context,
                    left_chromatogram,
                    right_chromatogram,
                    left_chromatogram_list
                        .default_data_processing_ref
                        .as_deref(),
                    right_chromatogram_list
                        .default_data_processing_ref
                        .as_deref(),
                    &format!("chromatogram[{index}]"),
                );
            }
        }
        (None, None) => {}
        _ => panic!("chromatogramList presence mismatch"),
    }
}

pub(crate) fn assert_mzml_structural_eq(left: &MzML, right: &MzML) {
    assert_eq!(left.run.id, right.run.id, "run id mismatch");
    assert_eq!(
        spectra(left).len(),
        spectra(right).len(),
        "spectrum count mismatch"
    );
    assert_eq!(
        chromatograms(left).len(),
        chromatograms(right).len(),
        "chromatogram count mismatch"
    );

    let left_ids: Vec<_> = spectra(left).iter().map(|s| s.id.as_str()).collect();
    let right_ids: Vec<_> = spectra(right).iter().map(|s| s.id.as_str()).collect();
    assert_eq!(left_ids, right_ids, "spectrum ids mismatch");

    let left_chrom_ids: Vec<_> = chromatograms(left).iter().map(|c| c.id.as_str()).collect();
    let right_chrom_ids: Vec<_> = chromatograms(right).iter().map(|c| c.id.as_str()).collect();
    assert_eq!(left_chrom_ids, right_chrom_ids, "chromatogram ids mismatch");
}

pub(crate) fn assert_referenceable_param_group_refs_resolved(
    refs: &[ReferenceableParamGroupRef],
    ref_group_ids: &BTreeSet<String>,
    context: &str,
) {
    for group_ref in refs {
        assert!(
            ref_group_ids.contains(group_ref.r#ref.as_str()),
            "{context} unresolved referenceableParamGroupRef: {}",
            group_ref.r#ref
        );
    }
}

pub(crate) fn assert_binary_data_array_lengths_consistent(
    arrays: &[BinaryDataArray],
    context: &str,
) {
    let mut canonical_len = None;
    for (index, array) in arrays.iter().enumerate() {
        let array_context = format!("{context} binaryDataArray[{index}]");
        if let (Some(binary), Some(array_length)) = (array.binary.as_ref(), array.array_length) {
            assert_eq!(
                binary.len(),
                array_length,
                "{array_context}: arrayLength does not match payload length"
            );
        }
        if let Some(binary) = array.binary.as_ref() {
            let len = binary.len();
            if len > 0 {
                if let Some(existing) = canonical_len {
                    assert_eq!(
                        len, existing,
                        "{array_context}: payload length mismatch across arrays"
                    );
                } else {
                    canonical_len = Some(len);
                }
            }
        }
    }
}

pub(crate) fn assert_all_refs_resolved(mzml: &MzML) {
    let source_file_ids = top_level_source_file_ids(mzml);
    let software_ids = top_level_software_ids(mzml);
    let dp_ids = top_level_dp_ids(mzml);
    let instrument_ids = top_level_instrument_ids(mzml);
    let sample_ids = top_level_sample_ids(mzml);
    let ref_group_ids = mzml
        .referenceable_param_group_list
        .as_ref()
        .map(|list| {
            set_of_ids(&list.referenceable_param_groups, |group| {
                Some(group.id.as_str())
            })
        })
        .unwrap_or_default();
    let scan_settings_ids = mzml
        .scan_settings_list
        .as_ref()
        .map(|list| set_of_ids(&list.scan_settings, |item| item.id.as_deref()))
        .unwrap_or_default();
    let spectrum_ids = set_of_ids(spectra(mzml), |s| Some(s.id.as_str()));

    assert_referenceable_param_group_refs_resolved(
        &mzml.run.referenceable_param_group_refs,
        &ref_group_ids,
        "run",
    );

    if let Some(file_description) = mzml.file_description.as_ref() {
        assert_referenceable_param_group_refs_resolved(
            &file_description.file_content.referenceable_param_group_refs,
            &ref_group_ids,
            "fileDescription.fileContent",
        );
        for (index, source_file) in file_description
            .source_file_list
            .source_file
            .iter()
            .enumerate()
        {
            assert_referenceable_param_group_refs_resolved(
                &source_file.referenceable_param_group_ref,
                &ref_group_ids,
                &format!("sourceFile[{index}]"),
            );
        }
        for (index, contact) in file_description.contacts.iter().enumerate() {
            assert_referenceable_param_group_refs_resolved(
                &contact.referenceable_param_group_refs,
                &ref_group_ids,
                &format!("contact[{index}]"),
            );
        }
    }

    if let Some(sample_list) = mzml.sample_list.as_ref() {
        for (index, sample) in sample_list.samples.iter().enumerate() {
            assert_referenceable_param_group_refs_resolved(
                &sample.referenceable_param_group_refs,
                &ref_group_ids,
                &format!("sample[{index}]"),
            );
        }
    }

    if let Some(r) = mzml.run.default_source_file_ref.as_deref() {
        assert!(
            source_file_ids.contains(r),
            "run.defaultSourceFileRef unresolved: {r}"
        );
    }
    if let Some(r) = mzml.run.default_instrument_configuration_ref.as_deref() {
        assert!(
            instrument_ids.contains(r),
            "run.defaultInstrumentConfigurationRef unresolved: {r}"
        );
    }
    if let Some(r) = mzml.run.sample_ref.as_deref() {
        assert!(sample_ids.contains(r), "run.sampleRef unresolved: {r}");
    }

    if let Some(sfrl) = mzml.run.source_file_ref_list.as_ref() {
        for sr in &sfrl.source_file_refs {
            assert!(
                source_file_ids.contains(sr.r#ref.as_str()),
                "run.sourceFileRefList unresolved ref: {}",
                sr.r#ref
            );
        }
    }

    if let Some(ssl) = mzml.scan_settings_list.as_ref() {
        for (index, ss) in ssl.scan_settings.iter().enumerate() {
            assert_referenceable_param_group_refs_resolved(
                &ss.referenceable_param_group_refs,
                &ref_group_ids,
                &format!("scanSettings[{index}]"),
            );
            if let Some(sfrl) = ss.source_file_ref_list.as_ref() {
                for sr in &sfrl.source_file_refs {
                    assert!(
                        source_file_ids.contains(sr.r#ref.as_str()),
                        "scanSettings sourceFileRef unresolved: {}",
                        sr.r#ref
                    );
                }
            }
            if let Some(icr) = ss.instrument_configuration_ref.as_deref() {
                assert!(
                    instrument_ids.contains(icr),
                    "scanSettings instrumentConfigurationRef unresolved: {icr}"
                );
            }
            if let Some(target_list) = ss.target_list.as_ref() {
                for (target_index, target) in target_list.targets.iter().enumerate() {
                    assert_referenceable_param_group_refs_resolved(
                        &target.referenceable_param_group_refs,
                        &ref_group_ids,
                        &format!("scanSettings[{index}].target[{target_index}]"),
                    );
                }
            }
        }
    }

    if let Some(il) = mzml.instrument_list.as_ref() {
        for (index, ic) in il.instrument.iter().enumerate() {
            assert_referenceable_param_group_refs_resolved(
                &ic.referenceable_param_group_ref,
                &ref_group_ids,
                &format!("instrument[{index}]"),
            );
            if let Some(sr) = ic.software_ref.as_ref() {
                assert!(
                    software_ids.contains(sr.r#ref.as_str()),
                    "instrument softwareRef unresolved: {}",
                    sr.r#ref
                );
            }
            if let Some(scan_settings_ref) = ic.scan_settings_ref.as_ref() {
                assert!(
                    scan_settings_ids.contains(scan_settings_ref.r#ref.as_str()),
                    "instrument scanSettingsRef unresolved: {}",
                    scan_settings_ref.r#ref
                );
            }
            if let Some(component_list) = ic.component_list.as_ref() {
                for (component_index, component) in component_list.source.iter().enumerate() {
                    assert_referenceable_param_group_refs_resolved(
                        &component.referenceable_param_group_ref,
                        &ref_group_ids,
                        &format!("instrument[{index}].source[{component_index}]"),
                    );
                }
                for (component_index, component) in component_list.analyzer.iter().enumerate() {
                    assert_referenceable_param_group_refs_resolved(
                        &component.referenceable_param_group_ref,
                        &ref_group_ids,
                        &format!("instrument[{index}].analyzer[{component_index}]"),
                    );
                }
                for (component_index, component) in component_list.detector.iter().enumerate() {
                    assert_referenceable_param_group_refs_resolved(
                        &component.referenceable_param_group_ref,
                        &ref_group_ids,
                        &format!("instrument[{index}].detector[{component_index}]"),
                    );
                }
            }
        }
    }

    if let Some(dpl) = mzml.data_processing_list.as_ref() {
        for (index, dp) in dpl.data_processing.iter().enumerate() {
            if let Some(sr) = dp.software_ref.as_deref() {
                assert!(
                    software_ids.contains(sr),
                    "dataProcessing softwareRef unresolved: {sr}"
                );
            }
            for (method_index, pm) in dp.processing_method.iter().enumerate() {
                assert_referenceable_param_group_refs_resolved(
                    &pm.referenceable_param_group_ref,
                    &ref_group_ids,
                    &format!("dataProcessing[{index}].processingMethod[{method_index}]"),
                );
                if let Some(sr) = pm.software_ref.as_deref() {
                    assert!(
                        software_ids.contains(sr),
                        "processingMethod softwareRef unresolved: {sr}"
                    );
                }
            }
        }
    }

    let spectrum_default_dp = mzml
        .run
        .spectrum_list
        .as_ref()
        .and_then(|sl| sl.default_data_processing_ref.as_deref());
    let chromatogram_default_dp = mzml
        .run
        .chromatogram_list
        .as_ref()
        .and_then(|cl| cl.default_data_processing_ref.as_deref());

    for s in spectra(mzml) {
        assert_referenceable_param_group_refs_resolved(
            &s.referenceable_param_group_refs,
            &ref_group_ids,
            &format!("spectrum {}", s.id),
        );
        if let Some(sr) = s.source_file_ref.as_deref() {
            assert!(
                source_file_ids.contains(sr),
                "spectrum sourceFileRef unresolved: {sr}"
            );
        }
        if let Some(dpr) = s.data_processing_ref.as_deref().or(spectrum_default_dp) {
            assert!(
                dp_ids.contains(dpr),
                "spectrum dataProcessingRef unresolved: {dpr}"
            );
        }

        let scan_list = if let Some(sd) = s.spectrum_description.as_ref() {
            assert_referenceable_param_group_refs_resolved(
                &sd.referenceable_param_group_refs,
                &ref_group_ids,
                &format!("spectrumDescription {}", s.id),
            );
            sd.scan_list.as_ref()
        } else {
            s.scan_list.as_ref()
        };

        if let Some(scan_list) = scan_list {
            for scan in &scan_list.scans {
                assert_referenceable_param_group_refs_resolved(
                    &scan.referenceable_param_group_refs,
                    &ref_group_ids,
                    &format!("scan in spectrum {}", s.id),
                );
                if let Some(icr) = scan.instrument_configuration_ref.as_deref() {
                    assert!(
                        instrument_ids.contains(icr),
                        "scan instrumentConfigurationRef unresolved: {icr}"
                    );
                }
                if let Some(sfr) = scan.source_file_ref.as_deref() {
                    assert!(
                        source_file_ids.contains(sfr),
                        "scan sourceFileRef unresolved: {sfr}"
                    );
                }
            }
        }

        let precursor_list = if let Some(sd) = s.spectrum_description.as_ref() {
            sd.precursor_list.as_ref()
        } else {
            s.precursor_list.as_ref()
        };

        if let Some(precursor_list) = precursor_list {
            for p in &precursor_list.precursors {
                if let Some(sr) = p.spectrum_ref.as_deref() {
                    assert!(
                        spectrum_ids.contains(sr),
                        "precursor spectrumRef unresolved: {sr}"
                    );
                }
                if let Some(sfr) = p.source_file_ref.as_deref() {
                    assert!(
                        source_file_ids.contains(sfr),
                        "precursor sourceFileRef unresolved: {sfr}"
                    );
                }
                if let Some(isolation_window) = p.isolation_window.as_ref() {
                    assert_referenceable_param_group_refs_resolved(
                        &isolation_window.referenceable_param_group_refs,
                        &ref_group_ids,
                        &format!("precursor isolationWindow in spectrum {}", s.id),
                    );
                }
                if let Some(selected_ion_list) = p.selected_ion_list.as_ref() {
                    for selected_ion in &selected_ion_list.selected_ions {
                        assert_referenceable_param_group_refs_resolved(
                            &selected_ion.referenceable_param_group_refs,
                            &ref_group_ids,
                            &format!("selectedIon in spectrum {}", s.id),
                        );
                    }
                }
                if let Some(activation) = p.activation.as_ref() {
                    assert_referenceable_param_group_refs_resolved(
                        &activation.referenceable_param_group_refs,
                        &ref_group_ids,
                        &format!("activation in spectrum {}", s.id),
                    );
                }
            }
        }

        if let Some(product_list) = product_list_of_spectrum(s) {
            for product in &product_list.products {
                if let Some(sr) = product.spectrum_ref.as_deref() {
                    assert!(
                        spectrum_ids.contains(sr),
                        "product spectrumRef unresolved: {sr}"
                    );
                }
                if let Some(sfr) = product.source_file_ref.as_deref() {
                    assert!(
                        source_file_ids.contains(sfr),
                        "product sourceFileRef unresolved: {sfr}"
                    );
                }
                if let Some(isolation_window) = product.isolation_window.as_ref() {
                    assert_referenceable_param_group_refs_resolved(
                        &isolation_window.referenceable_param_group_refs,
                        &ref_group_ids,
                        &format!("product isolationWindow in spectrum {}", s.id),
                    );
                }
            }
        }

        if let Some(binary_data_array_list) = s.binary_data_array_list.as_ref() {
            for array in &binary_data_array_list.binary_data_arrays {
                assert_referenceable_param_group_refs_resolved(
                    &array.referenceable_param_group_refs,
                    &ref_group_ids,
                    &format!("binaryDataArray in spectrum {}", s.id),
                );
                if let Some(data_processing_ref) = array.data_processing_ref.as_deref() {
                    assert!(
                        dp_ids.contains(data_processing_ref),
                        "binaryDataArray dataProcessingRef unresolved: {data_processing_ref}"
                    );
                }
            }
        }
    }

    for c in chromatograms(mzml) {
        assert_referenceable_param_group_refs_resolved(
            &c.referenceable_param_group_refs,
            &ref_group_ids,
            &format!("chromatogram {}", c.id),
        );
        if let Some(dpr) = c.data_processing_ref.as_deref().or(chromatogram_default_dp) {
            assert!(
                dp_ids.contains(dpr),
                "chromatogram dataProcessingRef unresolved: {dpr}"
            );
        }

        if let Some(p) = c.precursor.as_ref() {
            if let Some(sr) = p.spectrum_ref.as_deref() {
                assert!(
                    spectrum_ids.contains(sr),
                    "chrom precursor spectrumRef unresolved: {sr}"
                );
            }
            if let Some(sfr) = p.source_file_ref.as_deref() {
                assert!(
                    source_file_ids.contains(sfr),
                    "chrom precursor sourceFileRef unresolved: {sfr}"
                );
            }
            if let Some(isolation_window) = p.isolation_window.as_ref() {
                assert_referenceable_param_group_refs_resolved(
                    &isolation_window.referenceable_param_group_refs,
                    &ref_group_ids,
                    &format!("chrom precursor isolationWindow {}", c.id),
                );
            }
        }

        if let Some(p) = c.product.as_ref() {
            if let Some(sr) = p.spectrum_ref.as_deref() {
                assert!(
                    spectrum_ids.contains(sr),
                    "chrom product spectrumRef unresolved: {sr}"
                );
            }
            if let Some(sfr) = p.source_file_ref.as_deref() {
                assert!(
                    source_file_ids.contains(sfr),
                    "chrom product sourceFileRef unresolved: {sfr}"
                );
            }
            if let Some(isolation_window) = p.isolation_window.as_ref() {
                assert_referenceable_param_group_refs_resolved(
                    &isolation_window.referenceable_param_group_refs,
                    &ref_group_ids,
                    &format!("chrom product isolationWindow {}", c.id),
                );
            }
        }

        if let Some(binary_data_array_list) = c.binary_data_array_list.as_ref() {
            for array in &binary_data_array_list.binary_data_arrays {
                assert_referenceable_param_group_refs_resolved(
                    &array.referenceable_param_group_refs,
                    &ref_group_ids,
                    &format!("binaryDataArray in chromatogram {}", c.id),
                );
                if let Some(data_processing_ref) = array.data_processing_ref.as_deref() {
                    assert!(
                        dp_ids.contains(data_processing_ref),
                        "chromatogram binaryDataArray dataProcessingRef unresolved: \
                         {data_processing_ref}"
                    );
                }
            }
        }
    }
}

pub(crate) fn assert_semantic_roundtrip_via_xml(src: &MzML, context: &str) {
    let xml = ionic::mzml::bin_to_mzml::bin_to_mzml(src)
        .unwrap_or_else(|e| panic!("bin_to_mzml failed for {context}: {e}"));
    let reparsed = ionic::mzml::parse_mzml::parse_mzml(&xml)
        .unwrap_or_else(|e| panic!("reparse failed for {context}: {e}"));
    assert_mzml_semantic_eq(src, &reparsed);
}

pub(crate) fn assert_semantic_roundtrip_via_ion(src: &MzML, compression_level: u8, context: &str) {
    let bytes = super::encode_to_ion(src, compression_level, false);
    let decoded =
        super::decode_ion(&bytes).unwrap_or_else(|e| panic!("decode failed for {context}: {e}"));
    assert_mzml_semantic_eq(src, &decoded);
}

pub(crate) fn assert_semantic_roundtrip_full_pipeline(
    src: &MzML,
    compression_level: u8,
    context: &str,
) {
    let bytes = super::encode_to_ion(src, compression_level, false);
    let decoded =
        super::decode_ion(&bytes).unwrap_or_else(|e| panic!("decode failed for {context}: {e}"));
    let xml = ionic::mzml::bin_to_mzml::bin_to_mzml(&decoded)
        .unwrap_or_else(|e| panic!("bin_to_mzml failed for {context}: {e}"));
    let reparsed = ionic::mzml::parse_mzml::parse_mzml(&xml)
        .unwrap_or_else(|e| panic!("reparse failed for {context}: {e}"));
    assert_mzml_semantic_eq(src, &reparsed);
}

pub(crate) fn assert_index_offsets_match_model(indexed: &IndexedmzML, context: &str) {
    let spectrum_ids: Vec<_> = spectra(&indexed.mzml)
        .iter()
        .map(|s| s.id.clone())
        .collect();
    let chromatogram_ids: Vec<_> = chromatograms(&indexed.mzml)
        .iter()
        .map(|c| c.id.clone())
        .collect();

    assert_eq!(
        indexed.index_list.spectrum.len(),
        spectrum_ids.len(),
        "{context}: indexed spectrum count mismatch"
    );
    assert_eq!(
        indexed.index_list.chromatogram.len(),
        chromatogram_ids.len(),
        "{context}: indexed chromatogram count mismatch"
    );

    for (index, (offset, expected_id)) in indexed
        .index_list
        .spectrum
        .iter()
        .zip(spectrum_ids.iter())
        .enumerate()
    {
        assert_eq!(
            offset.id_ref.as_deref(),
            Some(expected_id.as_str()),
            "{context}: indexed spectrum id mismatch at {index}"
        );
        assert!(
            offset.offset > 0,
            "{context}: indexed spectrum offset {index} is zero"
        );
    }

    for (index, (offset, expected_id)) in indexed
        .index_list
        .chromatogram
        .iter()
        .zip(chromatogram_ids.iter())
        .enumerate()
    {
        assert_eq!(
            offset.id_ref.as_deref(),
            Some(expected_id.as_str()),
            "{context}: indexed chromatogram id mismatch at {index}"
        );
        assert!(
            offset.offset > 0,
            "{context}: indexed chromatogram offset {index} is zero"
        );
    }

    let mut previous = 0_u64;
    for (index, offset) in indexed.index_list.spectrum.iter().enumerate() {
        assert!(
            offset.offset >= previous,
            "{context}: indexed spectrum offsets are not monotonic at {index}"
        );
        previous = offset.offset;
    }
    previous = 0;
    for (index, offset) in indexed.index_list.chromatogram.iter().enumerate() {
        assert!(
            offset.offset >= previous,
            "{context}: indexed chromatogram offsets are not monotonic at {index}"
        );
        previous = offset.offset;
    }

    if !indexed.index_list.spectrum.is_empty() || !indexed.index_list.chromatogram.is_empty() {
        assert!(
            indexed.index_list_offset.is_some(),
            "{context}: indexListOffset missing despite populated index entries"
        );
    }
}
