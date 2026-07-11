use crate::{
    accessions::{FLOAT_32BIT, FLOAT_64BIT, format_accession},
    ion::{
        attr_meta::{
            ACC_ATTR_COUNT, ACC_ATTR_CV_FULL_NAME, ACC_ATTR_CV_URI, ACC_ATTR_CV_VERSION,
            ACC_ATTR_DEFAULT_INSTRUMENT_CONFIGURATION_REF, ACC_ATTR_DEFAULT_SOURCE_FILE_REF,
            ACC_ATTR_ID, ACC_ATTR_INSTRUMENT_CONFIGURATION_REF, ACC_ATTR_LABEL,
            ACC_ATTR_LOCATION, ACC_ATTR_NAME, ACC_ATTR_ORDER, ACC_ATTR_REF, ACC_ATTR_SAMPLE_REF,
            ACC_ATTR_SCAN_SETTINGS_REF, ACC_ATTR_SOFTWARE_REF, ACC_ATTR_START_TIME_STAMP,
            ACC_ATTR_VERSION,
        },
        utilities::EmitAttributes,
    },
    mzml::{
        schema::TagId,
        structs::{
            BinaryDataArray, BinaryDataArrayList, Chromatogram, CvParam, MzML, Precursor,
            PrecursorList, Product, ProductList, ReferenceableParamGroupRef, ScanList, Spectrum,
            SpectrumDescription, UserParam,
        },
    },
};

use super::{
    ArrayPolicy, GlobalCounts, MetaParamBuffer, MetaParamWriter, PackedMeta, PackedMetaBuilder,
    TraversalContext, append_meta_buffer, array_type_accession_from_binary_data_array,
    parse_accession_tail_raw,
};

pub(crate) trait MzmlListItem: EmitAttributes {
    fn list_tag() -> TagId;
    fn item_tag() -> TagId;
    fn has_explicit_index(&self) -> bool;
    fn cv_params(&self) -> &[CvParam];
    fn user_params(&self) -> &[UserParam];
    fn group_refs(&self) -> &[ReferenceableParamGroupRef];
    fn flatten_children(
        &self,
        writer: &mut MetaParamWriter<'_>,
        item_id: u32,
        context: &mut TraversalContext,
        policy: ArrayPolicy,
    );
}

impl MzmlListItem for Spectrum {
    fn list_tag() -> TagId {
        TagId::SpectrumList
    }
    fn item_tag() -> TagId {
        TagId::Spectrum
    }
    fn has_explicit_index(&self) -> bool {
        self.index.is_some()
    }
    fn cv_params(&self) -> &[CvParam] {
        &self.cv_params
    }
    fn user_params(&self) -> &[UserParam] {
        &self.user_params
    }
    fn group_refs(&self) -> &[ReferenceableParamGroupRef] {
        &self.referenceable_param_group_refs
    }
    fn flatten_children(
        &self,
        writer: &mut MetaParamWriter<'_>,
        id: u32,
        context: &mut TraversalContext,
        policy: ArrayPolicy,
    ) {
        flatten_spectrum_children(writer, self, id, context, policy);
    }
}

impl MzmlListItem for Chromatogram {
    fn list_tag() -> TagId {
        TagId::ChromatogramList
    }
    fn item_tag() -> TagId {
        TagId::Chromatogram
    }
    fn has_explicit_index(&self) -> bool {
        true
    }
    fn cv_params(&self) -> &[CvParam] {
        &self.cv_params
    }
    fn user_params(&self) -> &[UserParam] {
        &self.user_params
    }
    fn group_refs(&self) -> &[ReferenceableParamGroupRef] {
        &self.referenceable_param_group_refs
    }
    fn flatten_children(
        &self,
        writer: &mut MetaParamWriter<'_>,
        id: u32,
        context: &mut TraversalContext,
        policy: ArrayPolicy,
    ) {
        flatten_chromatogram_children(writer, self, id, context, policy);
    }
}

fn flatten_spectrum_children(
    writer: &mut MetaParamWriter<'_>,
    spectrum: &Spectrum,
    spectrum_id: u32,
    context: &mut TraversalContext,
    policy: ArrayPolicy,
) {
    if let Some(desc) = &spectrum.spectrum_description {
        flatten_legacy_spectrum_description(writer, desc, spectrum_id, context);
    }
    flatten_scan_list_opt(writer, spectrum.scan_list.as_ref(), spectrum_id, context);
    flatten_precursor_list_opt(
        writer,
        spectrum.precursor_list.as_ref(),
        spectrum_id,
        context,
    );
    flatten_product_list_opt(writer, spectrum.product_list.as_ref(), spectrum_id, context);
    flatten_binary_data_array_list(
        writer,
        spectrum.binary_data_array_list.as_ref(),
        spectrum_id,
        context,
        policy,
    );
}

fn flatten_legacy_spectrum_description(
    writer: &mut MetaParamWriter<'_>,
    desc: &SpectrumDescription,
    spectrum_id: u32,
    context: &mut TraversalContext,
) {
    let desc_id = context.alloc();
    writer.touch(TagId::SpectrumDescription, desc_id, spectrum_id);
    writer.push_ref_group_params(desc_id, &desc.referenceable_param_group_refs, context);
    writer.push_cv_and_user_params(desc_id, spectrum_id, &desc.cv_params, &desc.user_params);
    flatten_scan_list_opt(writer, desc.scan_list.as_ref(), desc_id, context);
    flatten_precursor_list_opt(writer, desc.precursor_list.as_ref(), desc_id, context);
    flatten_product_list_opt(writer, desc.product_list.as_ref(), desc_id, context);
}

fn flatten_chromatogram_children(
    writer: &mut MetaParamWriter<'_>,
    chrom: &Chromatogram,
    chrom_id: u32,
    context: &mut TraversalContext,
    policy: ArrayPolicy,
) {
    if let Some(p) = &chrom.precursor {
        flatten_precursor(writer, p, chrom_id, context);
    }
    if let Some(p) = &chrom.product {
        flatten_product(writer, p, chrom_id, context);
    }
    flatten_binary_data_array_list(
        writer,
        chrom.binary_data_array_list.as_ref(),
        chrom_id,
        context,
        policy,
    );
}

fn flatten_scan_list_opt(
    writer: &mut MetaParamWriter<'_>,
    sl: Option<&ScanList>,
    parent: u32,
    context: &mut TraversalContext,
) {
    let Some(sl) = sl else { return };
    let sl_id = context.alloc();
    writer.push_optional_u32_attr(
        TagId::ScanList,
        sl_id,
        parent,
        ACC_ATTR_COUNT,
        Some(sl.scans.len() as u32),
    );
    writer.push_ref_group_params(sl_id, &sl.referenceable_param_group_refs, context);
    writer.push_cv_and_user_params(sl_id, parent, &sl.cv_params, &sl.user_params);
    flatten_scan_list(writer, sl, sl_id, context);
}

fn flatten_precursor_list_opt(
    writer: &mut MetaParamWriter<'_>,
    pl: Option<&PrecursorList>,
    parent: u32,
    context: &mut TraversalContext,
) {
    let Some(pl) = pl else { return };
    let pl_id = context.alloc();
    writer.push_optional_u32_attr(
        TagId::PrecursorList,
        pl_id,
        parent,
        ACC_ATTR_COUNT,
        Some(pl.precursors.len() as u32),
    );
    writer.push_cv_and_user_params(pl_id, parent, &pl.cv_params, &pl.user_params);
    for p in &pl.precursors {
        flatten_precursor(writer, p, pl_id, context);
    }
}

fn flatten_product_list_opt(
    writer: &mut MetaParamWriter<'_>,
    pl: Option<&ProductList>,
    parent: u32,
    context: &mut TraversalContext,
) {
    let Some(pl) = pl else { return };
    let pl_id = context.alloc();
    writer.push_optional_u32_attr(
        TagId::ProductList,
        pl_id,
        parent,
        ACC_ATTR_COUNT,
        Some(pl.products.len() as u32),
    );
    writer.push_cv_and_user_params(pl_id, parent, &pl.cv_params, &pl.user_params);
    for p in &pl.products {
        flatten_product(writer, p, pl_id, context);
    }
}

fn flatten_binary_data_array_list(
    writer: &mut MetaParamWriter<'_>,
    bda_list: Option<&BinaryDataArrayList>,
    parent_id: u32,
    context: &mut TraversalContext,
    policy: ArrayPolicy,
) {
    let Some(list) = bda_list else { return };
    let list_id = context.alloc();
    writer.push_optional_u32_attr(
        TagId::BinaryDataArrayList,
        list_id,
        parent_id,
        ACC_ATTR_COUNT,
        Some(list.binary_data_arrays.len() as u32),
    );
    for bda in &list.binary_data_arrays {
        let bda_id = context.alloc();
        writer.touch(TagId::BinaryDataArray, bda_id, list_id);
        writer.push_schema_attrs(TagId::BinaryDataArray, bda_id, list_id, bda);
        writer.push_ref_group_params(bda_id, &bda.referenceable_param_group_refs, context);
        emit_binary_data_array_cv_params(writer, bda_id, list_id, bda, policy);
    }
}

fn flatten_precursor(
    writer: &mut MetaParamWriter<'_>,
    precursor: &Precursor,
    parent_id: u32,
    context: &mut TraversalContext,
) {
    let p_id = context.alloc();
    writer.touch(TagId::Precursor, p_id, parent_id);
    writer.push_schema_attrs(TagId::Precursor, p_id, parent_id, precursor);
    if let Some(iw) = &precursor.isolation_window {
        let iw_id = context.alloc();
        writer.touch(TagId::IsolationWindow, iw_id, p_id);
        writer.push_ref_group_params(iw_id, &iw.referenceable_param_group_refs, context);
        writer.push_cv_and_user_params(iw_id, p_id, &iw.cv_params, &iw.user_params);
    }
    if let Some(sil) = &precursor.selected_ion_list {
        let sil_id = context.alloc();
        writer.push_optional_u32_attr(
            TagId::SelectedIonList,
            sil_id,
            p_id,
            ACC_ATTR_COUNT,
            Some(sil.selected_ions.len() as u32),
        );
        for si in &sil.selected_ions {
            let si_id = context.alloc();
            writer.touch(TagId::SelectedIon, si_id, sil_id);
            writer.push_ref_group_params(si_id, &si.referenceable_param_group_refs, context);
            writer.push_cv_and_user_params(si_id, sil_id, &si.cv_params, &si.user_params);
        }
    }
    if let Some(act) = &precursor.activation {
        let act_id = context.alloc();
        writer.touch(TagId::Activation, act_id, p_id);
        writer.push_ref_group_params(act_id, &act.referenceable_param_group_refs, context);
        writer.push_cv_and_user_params(act_id, p_id, &act.cv_params, &act.user_params);
    }
}

fn flatten_product(
    writer: &mut MetaParamWriter<'_>,
    product: &Product,
    parent_id: u32,
    context: &mut TraversalContext,
) {
    let prod_id = context.alloc();
    writer.touch(TagId::Product, prod_id, parent_id);
    writer.push_schema_attrs(TagId::Product, prod_id, parent_id, product);
    writer.push_cv_and_user_params(prod_id, parent_id, &product.cv_params, &product.user_params);
    if let Some(iw) = &product.isolation_window {
        let iw_id = context.alloc();
        writer.touch(TagId::IsolationWindow, iw_id, prod_id);
        writer.push_ref_group_params(iw_id, &iw.referenceable_param_group_refs, context);
        writer.push_cv_and_user_params(iw_id, prod_id, &iw.cv_params, &iw.user_params);
    }
}

fn flatten_scan_list(
    writer: &mut MetaParamWriter<'_>,
    scan_list: &ScanList,
    sl_id: u32,
    context: &mut TraversalContext,
) {
    for scan in &scan_list.scans {
        let scan_id = context.alloc();
        writer.touch(TagId::Scan, scan_id, sl_id);
        writer.push_schema_attrs(TagId::Scan, scan_id, sl_id, scan);
        writer.push_ref_group_params(scan_id, &scan.referenceable_param_group_refs, context);
        writer.push_cv_and_user_params(scan_id, sl_id, &scan.cv_params, &scan.user_params);
        if let Some(swl) = &scan.scan_window_list {
            let swl_id = context.alloc();
            writer.push_optional_u32_attr(
                TagId::ScanWindowList,
                swl_id,
                scan_id,
                ACC_ATTR_COUNT,
                Some(swl.scan_windows.len() as u32),
            );
            for sw in &swl.scan_windows {
                let sw_id = context.alloc();
                writer.touch(TagId::ScanWindow, sw_id, swl_id);
                writer.push_cv_and_user_params(sw_id, swl_id, &sw.cv_params, &sw.user_params);
            }
        }
    }
}

fn make_float_precision_cv_param(accession_tail: u32) -> CvParam {
    let name = if accession_tail == FLOAT_32BIT {
        "32-bit float"
    } else {
        "64-bit float"
    };
    CvParam {
        cv_ref: Some("MS".to_string()),
        accession: Some(format_accession(accession_tail)),
        name: name.to_string(),
        value: None,
        unit_cv_ref: None,
        unit_name: None,
        unit_accession: None,
    }
}

fn emit_binary_data_array_cv_params(
    writer: &mut MetaParamWriter<'_>,
    bda_node_id: u32,
    bda_list_node_id: u32,
    bda: &BinaryDataArray,
    policy: ArrayPolicy,
) {
    let array_acc = array_type_accession_from_binary_data_array(bda);
    if !(policy.force_f32 && policy.is_xy_array(array_acc)) {
        writer.push_many(
            TagId::CvParam,
            bda_node_id,
            bda_list_node_id,
            &bda.cv_params,
        );
        return;
    }
    let mut precision_written = false;
    for cv in &bda.cv_params {
        let tail = parse_accession_tail_raw(cv.accession.as_deref());
        if tail == FLOAT_32BIT || tail == FLOAT_64BIT {
            if !precision_written {
                writer.push_one(
                    TagId::CvParam,
                    bda_node_id,
                    bda_list_node_id,
                    make_float_precision_cv_param(FLOAT_32BIT),
                );
                precision_written = true;
            }
        } else {
            writer.push_one(TagId::CvParam, bda_node_id, bda_list_node_id, cv.clone());
        }
    }
    if !precision_written {
        writer.push_one(
            TagId::CvParam,
            bda_node_id,
            bda_list_node_id,
            make_float_precision_cv_param(FLOAT_32BIT),
        );
    }
}

pub(super) fn pack_global_meta(
    mzml: &MzML,
    context: &mut TraversalContext,
) -> (PackedMeta, GlobalCounts) {
    let mut buffers: Vec<MetaParamBuffer> = Vec::new();

    let n_file_description = append_file_description_meta(mzml, context, &mut buffers);
    let n_run = append_run_meta(mzml, context, &mut buffers);
    let n_ref_param_groups = append_ref_param_groups_meta(mzml, context, &mut buffers);
    let n_samples = append_samples_meta(mzml, context, &mut buffers);
    let n_instrument_configs = append_instruments_meta(mzml, context, &mut buffers);
    let n_software = append_software_list_meta(mzml, context, &mut buffers);
    let n_data_processing = append_data_processing_list_meta(mzml, context, &mut buffers);
    let n_acquisition_settings = append_scan_settings_list_meta(mzml, context, &mut buffers);
    let n_cvs = append_cv_list_meta(mzml, context, &mut buffers);

    let counts = GlobalCounts {
        n_file_description,
        n_run,
        n_ref_param_groups,
        n_samples,
        n_instrument_configs,
        n_software,
        n_data_processing,
        n_acquisition_settings,
        n_cvs,
    };
    let mut builder = PackedMetaBuilder::new();
    for buffer in &buffers {
        builder.flush_buffer(buffer);
    }
    (builder.build(), counts)
}

fn append_file_description_meta(
    mzml: &MzML,
    context: &mut TraversalContext,
    buffers: &mut Vec<MetaParamBuffer>,
) -> u32 {
    let Some(fd) = &mzml.file_description else {
        return 0;
    };
    append_meta_buffer(buffers, |writer| {
        let fd_id = context.alloc();
        let fc_id = context.alloc();
        let sfl_id = context.alloc();

        writer.touch(TagId::FileDescription, fd_id, 0);
        writer.touch(TagId::FileContent, fc_id, fd_id);
        writer.push_ref_group_params(
            fc_id,
            &fd.file_content.referenceable_param_group_refs,
            context,
        );
        writer.push_cv_and_user_params(
            fc_id,
            fd_id,
            &fd.file_content.cv_params,
            &fd.file_content.user_params,
        );

        writer.touch(TagId::SourceFileList, sfl_id, fd_id);
        writer.push_optional_u32_attr(
            TagId::SourceFileList,
            sfl_id,
            fd_id,
            ACC_ATTR_COUNT,
            Some(fd.source_file_list.source_file.len() as u32),
        );

        for sf in &fd.source_file_list.source_file {
            let sf_id = context.alloc();
            writer.touch(TagId::SourceFile, sf_id, sfl_id);
            writer.push_str_attr(TagId::SourceFile, sf_id, sfl_id, ACC_ATTR_ID, &sf.id);
            writer.push_str_attr(TagId::SourceFile, sf_id, sfl_id, ACC_ATTR_NAME, &sf.name);
            writer.push_str_attr(
                TagId::SourceFile,
                sf_id,
                sfl_id,
                ACC_ATTR_LOCATION,
                &sf.location,
            );
            writer.push_ref_group_params(sf_id, &sf.referenceable_param_group_ref, context);
            writer.push_cv_and_user_params(sf_id, sfl_id, &sf.cv_param, &sf.user_param);
        }
        for contact in &fd.contacts {
            let c_id = context.alloc();
            writer.touch(TagId::Contact, c_id, fd_id);
            writer.push_ref_group_params(c_id, &contact.referenceable_param_group_refs, context);
            writer.push_cv_and_user_params(c_id, fd_id, &contact.cv_params, &contact.user_params);
        }
    });
    1
}

fn append_run_meta(
    mzml: &MzML,
    context: &mut TraversalContext,
    buffers: &mut Vec<MetaParamBuffer>,
) -> u32 {
    let run = &mzml.run;
    append_meta_buffer(buffers, |writer| {
        let run_id = context.alloc();
        writer.touch(TagId::Run, run_id, 0);
        writer.push_str_attr(TagId::Run, run_id, 0, ACC_ATTR_ID, &run.id);
        if let Some(ts) = run.start_time_stamp.as_deref() {
            writer.push_str_attr(TagId::Run, run_id, 0, ACC_ATTR_START_TIME_STAMP, ts);
        }
        if let Some(r) = run.default_instrument_configuration_ref.as_deref() {
            writer.push_str_attr(
                TagId::Run,
                run_id,
                0,
                ACC_ATTR_DEFAULT_INSTRUMENT_CONFIGURATION_REF,
                r,
            );
        }
        if let Some(r) = run.default_source_file_ref.as_deref() {
            writer.push_str_attr(TagId::Run, run_id, 0, ACC_ATTR_DEFAULT_SOURCE_FILE_REF, r);
        }
        if let Some(r) = run.sample_ref.as_deref() {
            writer.push_str_attr(TagId::Run, run_id, 0, ACC_ATTR_SAMPLE_REF, r);
        }
        if let Some(sfrl) = &run.source_file_ref_list {
            let sfrl_id = context.alloc();
            writer.touch(TagId::SourceFileRefList, sfrl_id, run_id);
            writer.push_optional_u32_attr(
                TagId::SourceFileRefList,
                sfrl_id,
                run_id,
                ACC_ATTR_COUNT,
                Some(sfrl.source_file_refs.len() as u32),
            );
            for sfr in &sfrl.source_file_refs {
                let sfr_id = context.alloc();
                writer.touch(TagId::SourceFileRef, sfr_id, sfrl_id);
                writer.push_str_attr(
                    TagId::SourceFileRef,
                    sfr_id,
                    sfrl_id,
                    ACC_ATTR_REF,
                    &sfr.r#ref,
                );
            }
        }
        for gr in &run.referenceable_param_group_refs {
            let gref_id = context.alloc();
            writer.touch(TagId::ReferenceableParamGroupRef, gref_id, run_id);
            writer.push_str_attr(
                TagId::ReferenceableParamGroupRef,
                gref_id,
                run_id,
                ACC_ATTR_REF,
                &gr.r#ref,
            );
        }
        writer.push_cv_and_user_params(run_id, 0, &run.cv_params, &run.user_params);
    });
    1
}

fn append_ref_param_groups_meta(
    mzml: &MzML,
    context: &mut TraversalContext,
    buffers: &mut Vec<MetaParamBuffer>,
) -> u32 {
    let Some(list) = &mzml.referenceable_param_group_list else {
        return 0;
    };
    for group in &list.referenceable_param_groups {
        append_meta_buffer(buffers, |writer| {
            let gid = context.alloc();
            writer.touch(TagId::ReferenceableParamGroup, gid, 0);
            writer.push_str_attr(
                TagId::ReferenceableParamGroup,
                gid,
                0,
                ACC_ATTR_ID,
                &group.id,
            );
            writer.push_cv_and_user_params(gid, 0, &group.cv_params, &group.user_params);
        });
    }
    list.referenceable_param_groups.len() as u32
}

fn append_samples_meta(
    mzml: &MzML,
    context: &mut TraversalContext,
    buffers: &mut Vec<MetaParamBuffer>,
) -> u32 {
    let Some(list) = &mzml.sample_list else {
        return 0;
    };
    let list_id = context.alloc();
    for (i, sample) in list.samples.iter().enumerate() {
        append_meta_buffer(buffers, |writer| {
            if i == 0 {
                writer.touch(TagId::SampleList, list_id, 0);
                writer.push_cv_and_user_params(list_id, 0, &list.cv_params, &list.user_params);
            }
            let sid = context.alloc();
            writer.touch(TagId::Sample, sid, list_id);
            writer.push_str_attr(TagId::Sample, sid, list_id, ACC_ATTR_ID, &sample.id);
            writer.push_str_attr(TagId::Sample, sid, list_id, ACC_ATTR_NAME, &sample.name);
            writer.push_ref_group_params(sid, &sample.referenceable_param_group_refs, context);
            writer.push_cv_and_user_params(sid, list_id, &sample.cv_params, &sample.user_params);
        });
    }
    list.samples.len() as u32
}

#[allow(clippy::too_many_arguments)]
fn emit_instrument_component(
    writer: &mut MetaParamWriter<'_>,
    tag: TagId,
    order: Option<u32>,
    parent_id: u32,
    group_refs: &[ReferenceableParamGroupRef],
    cv_params: &[CvParam],
    user_params: &[UserParam],
    context: &mut TraversalContext,
) {
    let comp_id = context.alloc();
    writer.touch(tag, comp_id, parent_id);
    writer.push_optional_u32_attr(tag, comp_id, parent_id, ACC_ATTR_ORDER, order);
    writer.push_ref_group_params(comp_id, group_refs, context);
    writer.push_cv_and_user_params(comp_id, parent_id, cv_params, user_params);
}

fn append_instruments_meta(
    mzml: &MzML,
    context: &mut TraversalContext,
    buffers: &mut Vec<MetaParamBuffer>,
) -> u32 {
    let Some(list) = &mzml.instrument_list else {
        return 0;
    };
    for inst in &list.instrument {
        append_meta_buffer(buffers, |writer| {
            let id = context.alloc();
            writer.touch(TagId::Instrument, id, 0);
            writer.push_str_attr(TagId::Instrument, id, 0, ACC_ATTR_ID, &inst.id);
            if let Some(ssr) = &inst.scan_settings_ref {
                writer.push_str_attr(
                    TagId::Instrument,
                    id,
                    0,
                    ACC_ATTR_SCAN_SETTINGS_REF,
                    &ssr.r#ref,
                );
            }
            writer.push_ref_group_params(id, &inst.referenceable_param_group_ref, context);
            writer.push_cv_and_user_params(id, 0, &inst.cv_param, &inst.user_param);
            if let Some(sw) = &inst.software_ref {
                let sw_id = context.alloc();
                writer.touch(TagId::SoftwareRef, sw_id, id);
                writer.push_str_attr(TagId::SoftwareRef, sw_id, id, ACC_ATTR_REF, &sw.r#ref);
            }
            if let Some(cl) = &inst.component_list {
                for s in &cl.source {
                    emit_instrument_component(
                        writer,
                        TagId::ComponentSource,
                        s.order,
                        id,
                        &s.referenceable_param_group_ref,
                        &s.cv_param,
                        &s.user_param,
                        context,
                    );
                }
                for a in &cl.analyzer {
                    emit_instrument_component(
                        writer,
                        TagId::ComponentAnalyzer,
                        a.order,
                        id,
                        &a.referenceable_param_group_ref,
                        &a.cv_param,
                        &a.user_param,
                        context,
                    );
                }
                for d in &cl.detector {
                    emit_instrument_component(
                        writer,
                        TagId::ComponentDetector,
                        d.order,
                        id,
                        &d.referenceable_param_group_ref,
                        &d.cv_param,
                        &d.user_param,
                        context,
                    );
                }
            }
        });
    }
    list.instrument.len() as u32
}

fn append_software_list_meta(
    mzml: &MzML,
    context: &mut TraversalContext,
    buffers: &mut Vec<MetaParamBuffer>,
) -> u32 {
    let Some(list) = &mzml.software_list else {
        return 0;
    };
    for sw in &list.software {
        append_meta_buffer(buffers, |writer| {
            let swid = context.alloc();
            writer.touch(TagId::Software, swid, 0);
            writer.push_str_attr(TagId::Software, swid, 0, ACC_ATTR_ID, &sw.id);
            let version = sw
                .version
                .as_deref()
                .or_else(|| sw.software_param.first().and_then(|p| p.version.as_deref()));
            if let Some(v) = version {
                writer.push_str_attr(TagId::Software, swid, 0, ACC_ATTR_VERSION, v);
            }
            writer.push_ref_group_params(swid, &sw.referenceable_param_group_refs, context);
            for sp in &sw.software_param {
                let pid = context.alloc();
                writer.touch(TagId::SoftwareParam, pid, swid);
                writer.push_one(
                    TagId::SoftwareParam,
                    pid,
                    swid,
                    CvParam {
                        cv_ref: sp.cv_ref.clone(),
                        accession: Some(sp.accession.clone()),
                        name: sp.name.clone(),
                        value: Some(String::new()),
                        unit_cv_ref: None,
                        unit_name: None,
                        unit_accession: None,
                    },
                );
            }
            writer.push_cv_and_user_params(swid, 0, &sw.cv_param, &sw.user_params);
        });
    }
    list.software.len() as u32
}

fn append_data_processing_list_meta(
    mzml: &MzML,
    context: &mut TraversalContext,
    buffers: &mut Vec<MetaParamBuffer>,
) -> u32 {
    let Some(list) = &mzml.data_processing_list else {
        return 0;
    };
    for dp in &list.data_processing {
        append_meta_buffer(buffers, |writer| {
            let dp_id = context.alloc();
            writer.touch(TagId::DataProcessing, dp_id, 0);
            writer.push_str_attr(TagId::DataProcessing, dp_id, 0, ACC_ATTR_ID, &dp.id);
            if let Some(sw) = dp.software_ref.as_deref() {
                writer.push_str_attr(TagId::DataProcessing, dp_id, 0, ACC_ATTR_SOFTWARE_REF, sw);
            }
            for pm in &dp.processing_method {
                let pm_id = context.alloc();
                writer.touch(TagId::ProcessingMethod, pm_id, dp_id);
                writer.push_optional_u32_attr(
                    TagId::ProcessingMethod,
                    pm_id,
                    dp_id,
                    ACC_ATTR_ORDER,
                    pm.order,
                );
                if let Some(sw) = pm.software_ref.as_deref() {
                    writer.push_str_attr(
                        TagId::ProcessingMethod,
                        pm_id,
                        dp_id,
                        ACC_ATTR_SOFTWARE_REF,
                        sw,
                    );
                }
                writer.push_ref_group_params(pm_id, &pm.referenceable_param_group_ref, context);
                writer.push_cv_and_user_params(pm_id, dp_id, &pm.cv_param, &pm.user_param);
            }
        });
    }
    list.data_processing.len() as u32
}

fn append_scan_settings_list_meta(
    mzml: &MzML,
    context: &mut TraversalContext,
    buffers: &mut Vec<MetaParamBuffer>,
) -> u32 {
    let Some(list) = &mzml.scan_settings_list else {
        return 0;
    };
    for ss in &list.scan_settings {
        append_meta_buffer(buffers, |writer| {
            let ss_id = context.alloc();
            writer.touch(TagId::ScanSettings, ss_id, 0);
            if let Some(id) = ss.id.as_deref() {
                writer.push_str_attr(TagId::ScanSettings, ss_id, 0, ACC_ATTR_ID, id);
            }
            if let Some(r) = ss.instrument_configuration_ref.as_deref() {
                writer.push_str_attr(
                    TagId::ScanSettings,
                    ss_id,
                    0,
                    ACC_ATTR_INSTRUMENT_CONFIGURATION_REF,
                    r,
                );
            }
            if let Some(sfrl) = &ss.source_file_ref_list {
                let sfrl_id = context.alloc();
                writer.touch(TagId::SourceFileRefList, sfrl_id, ss_id);
                writer.push_optional_u32_attr(
                    TagId::SourceFileRefList,
                    sfrl_id,
                    ss_id,
                    ACC_ATTR_COUNT,
                    Some(sfrl.source_file_refs.len() as u32),
                );
                for sfr in &sfrl.source_file_refs {
                    let sfr_id = context.alloc();
                    writer.touch(TagId::SourceFileRef, sfr_id, sfrl_id);
                    writer.push_str_attr(
                        TagId::SourceFileRef,
                        sfr_id,
                        sfrl_id,
                        ACC_ATTR_REF,
                        &sfr.r#ref,
                    );
                }
            }
            writer.push_ref_group_params(ss_id, &ss.referenceable_param_group_refs, context);
            writer.push_cv_and_user_params(ss_id, 0, &ss.cv_params, &ss.user_params);
            if let Some(tl) = &ss.target_list {
                for target in &tl.targets {
                    let t_id = context.alloc();
                    writer.touch(TagId::Target, t_id, ss_id);
                    writer.push_ref_group_params(
                        t_id,
                        &target.referenceable_param_group_refs,
                        context,
                    );
                    writer.push_cv_and_user_params(
                        t_id,
                        ss_id,
                        &target.cv_params,
                        &target.user_params,
                    );
                }
            }
        });
    }
    list.scan_settings.len() as u32
}

fn append_cv_list_meta(
    mzml: &MzML,
    context: &mut TraversalContext,
    buffers: &mut Vec<MetaParamBuffer>,
) -> u32 {
    let Some(cv_list) = &mzml.cv_list else {
        return 0;
    };
    if cv_list.cv.is_empty() {
        return 0;
    }
    let cv_list_id = context.alloc();
    let cv_count = cv_list.cv.len() as u32;
    for (i, cv) in cv_list.cv.iter().enumerate() {
        append_meta_buffer(buffers, |writer| {
            if i == 0 {
                writer.touch(TagId::CvList, cv_list_id, 0);
                writer.push_optional_u32_attr(
                    TagId::CvList,
                    cv_list_id,
                    0,
                    ACC_ATTR_COUNT,
                    Some(cv_count),
                );
            }
            let cv_id = context.alloc();
            writer.touch(TagId::Cv, cv_id, cv_list_id);
            writer.push_str_attr(TagId::Cv, cv_id, cv_list_id, ACC_ATTR_LABEL, &cv.id);
            if let Some(n) = cv.full_name.as_deref().filter(|s| !s.is_empty()) {
                writer.push_str_attr(TagId::Cv, cv_id, cv_list_id, ACC_ATTR_CV_FULL_NAME, n);
            }
            if let Some(v) = cv.version.as_deref().filter(|s| !s.is_empty()) {
                writer.push_str_attr(TagId::Cv, cv_id, cv_list_id, ACC_ATTR_CV_VERSION, v);
            }
            if let Some(u) = cv.uri.as_deref().filter(|s| !s.is_empty()) {
                writer.push_str_attr(TagId::Cv, cv_id, cv_list_id, ACC_ATTR_CV_URI, u);
            }
        });
    }
    cv_count
}
