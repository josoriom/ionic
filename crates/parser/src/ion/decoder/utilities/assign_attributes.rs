use crate::{
    ion::{
        attr_meta::{
            ACC_ATTR_ARRAY_LENGTH, ACC_ATTR_COUNT, ACC_ATTR_DATA_PROCESSING_REF,
            ACC_ATTR_DEFAULT_ARRAY_LENGTH, ACC_ATTR_DEFAULT_DATA_PROCESSING_REF,
            ACC_ATTR_ENCODED_LENGTH, ACC_ATTR_EXTERNAL_SPECTRUM_ID, ACC_ATTR_ID, ACC_ATTR_INDEX,
            ACC_ATTR_INSTRUMENT_CONFIGURATION_REF, ACC_ATTR_NATIVE_ID, ACC_ATTR_SCAN_NUMBER,
            ACC_ATTR_SOURCE_FILE_REF, ACC_ATTR_SPECTRUM_REF, ACC_ATTR_SPOT_ID, AccessionTail,
            CV_REF_ATTR,
        },
        decoder::decode::{Metadatum, MetadatumValue},
    },
    mzml::{
        schema::TagId,
        structs::{
            BinaryDataArray, Chromatogram, ChromatogramList, Precursor, Product, Scan, Spectrum,
            SpectrumList,
        },
    },
};

pub(crate) trait EmitAttributes {
    fn emit_attributes(&self, collector: &mut AttributeCollector<'_>);
}

pub(crate) struct AttributeCollector<'a> {
    out: &'a mut Vec<Metadatum>,
    tag_id: TagId,
    node_id: u32,
    parent_id: u32,
    next_item_index: u32,
}

impl<'a> AttributeCollector<'a> {
    #[inline]
    pub fn push_required_str(&mut self, tail: AccessionTail, value: &str) {
        self.out.push(Metadatum {
            item_index: self.next_item_index,
            id: self.node_id,
            parent_id: self.parent_id,
            tag_id: self.tag_id,
            accession: Some(format_accession(tail)),
            unit_accession: None,
            value: MetadatumValue::Text(value.to_owned()),
        });
        self.next_item_index += 1;
    }

    #[inline]
    pub fn push_str(&mut self, tail: AccessionTail, value: &str) {
        if !value.is_empty() {
            self.push_required_str(tail, value);
        }
    }

    #[inline]
    pub fn push_num(&mut self, tail: AccessionTail, value: f64) {
        self.out.push(Metadatum {
            item_index: self.next_item_index,
            id: self.node_id,
            parent_id: self.parent_id,
            tag_id: self.tag_id,
            accession: Some(format_accession(tail)),
            unit_accession: None,
            value: MetadatumValue::Number(value),
        });
        self.next_item_index += 1;
    }

    #[inline]
    pub fn push_opt_str(&mut self, tail: AccessionTail, value: Option<&str>) {
        if let Some(v) = value {
            self.push_str(tail, v);
        }
    }

    #[inline]
    pub fn push_opt_usize(&mut self, tail: AccessionTail, value: Option<usize>) {
        if let Some(v) = value {
            self.push_num(tail, v as f64);
        }
    }

    #[inline]
    pub fn push_opt_u32(&mut self, tail: AccessionTail, value: Option<u32>) {
        if let Some(v) = value {
            self.push_num(tail, f64::from(v));
        }
    }
}

pub(crate) fn assign_attributes_into<T: EmitAttributes>(
    value: &T,
    tag_id: TagId,
    node_id: u32,
    parent_id: u32,
    out: &mut Vec<Metadatum>,
) {
    let mut collector = AttributeCollector {
        out,
        tag_id,
        node_id,
        parent_id,
        next_item_index: 0,
    };
    value.emit_attributes(&mut collector);
}

#[cfg(test)]
pub(crate) fn assign_attributes<T: EmitAttributes>(
    value: &T,
    tag_id: TagId,
    node_id: u32,
    parent_id: u32,
) -> Vec<Metadatum> {
    let mut out = Vec::new();
    assign_attributes_into(value, tag_id, node_id, parent_id, &mut out);
    out
}

fn format_accession(tail: AccessionTail) -> String {
    use core::fmt::Write;
    let mut accession = String::with_capacity(CV_REF_ATTR.len() + 8);
    accession.push_str(CV_REF_ATTR);
    accession.push(':');
    write!(&mut accession, "{:07}", tail.raw()).expect("format accession tail");
    accession
}

impl EmitAttributes for SpectrumList {
    fn emit_attributes(&self, collector: &mut AttributeCollector<'_>) {
        collector.push_opt_usize(ACC_ATTR_COUNT, self.count);
        collector.push_opt_str(
            ACC_ATTR_DEFAULT_DATA_PROCESSING_REF,
            self.default_data_processing_ref.as_deref(),
        );
    }
}

impl EmitAttributes for Spectrum {
    fn emit_attributes(&self, collector: &mut AttributeCollector<'_>) {
        collector.push_required_str(ACC_ATTR_ID, &self.id);
        collector.push_opt_u32(ACC_ATTR_INDEX, self.index);
        collector.push_opt_u32(ACC_ATTR_SCAN_NUMBER, self.scan_number);
        collector.push_opt_str(ACC_ATTR_NATIVE_ID, self.native_id.as_deref());
        collector.push_opt_usize(ACC_ATTR_DEFAULT_ARRAY_LENGTH, self.default_array_length);
        collector.push_opt_str(
            ACC_ATTR_DATA_PROCESSING_REF,
            self.data_processing_ref.as_deref(),
        );
        collector.push_opt_str(ACC_ATTR_SOURCE_FILE_REF, self.source_file_ref.as_deref());
        collector.push_opt_str(ACC_ATTR_SPOT_ID, self.spot_id.as_deref());
    }
}

impl EmitAttributes for ChromatogramList {
    fn emit_attributes(&self, collector: &mut AttributeCollector<'_>) {
        collector.push_opt_usize(ACC_ATTR_COUNT, self.count);
        collector.push_opt_str(
            ACC_ATTR_DEFAULT_DATA_PROCESSING_REF,
            self.default_data_processing_ref.as_deref(),
        );
    }
}

impl EmitAttributes for Chromatogram {
    fn emit_attributes(&self, collector: &mut AttributeCollector<'_>) {
        collector.push_required_str(ACC_ATTR_ID, &self.id);
        collector.push_opt_u32(ACC_ATTR_INDEX, self.index);
        collector.push_opt_str(ACC_ATTR_NATIVE_ID, self.native_id.as_deref());
        collector.push_opt_usize(ACC_ATTR_DEFAULT_ARRAY_LENGTH, self.default_array_length);
        collector.push_opt_str(
            ACC_ATTR_DATA_PROCESSING_REF,
            self.data_processing_ref.as_deref(),
        );
    }
}

impl EmitAttributes for BinaryDataArray {
    fn emit_attributes(&self, collector: &mut AttributeCollector<'_>) {
        collector.push_opt_usize(ACC_ATTR_ARRAY_LENGTH, self.array_length);
        collector.push_opt_usize(ACC_ATTR_ENCODED_LENGTH, self.encoded_length);
        collector.push_opt_str(
            ACC_ATTR_DATA_PROCESSING_REF,
            self.data_processing_ref.as_deref(),
        );
    }
}

impl EmitAttributes for Precursor {
    fn emit_attributes(&self, collector: &mut AttributeCollector<'_>) {
        collector.push_opt_str(ACC_ATTR_SPECTRUM_REF, self.spectrum_ref.as_deref());
        collector.push_opt_str(ACC_ATTR_SOURCE_FILE_REF, self.source_file_ref.as_deref());
        collector.push_opt_str(
            ACC_ATTR_EXTERNAL_SPECTRUM_ID,
            self.external_spectrum_id.as_deref(),
        );
    }
}

impl EmitAttributes for Product {
    fn emit_attributes(&self, collector: &mut AttributeCollector<'_>) {
        collector.push_opt_str(ACC_ATTR_SPECTRUM_REF, self.spectrum_ref.as_deref());
        collector.push_opt_str(ACC_ATTR_SOURCE_FILE_REF, self.source_file_ref.as_deref());
        collector.push_opt_str(
            ACC_ATTR_EXTERNAL_SPECTRUM_ID,
            self.external_spectrum_id.as_deref(),
        );
    }
}

impl EmitAttributes for Scan {
    fn emit_attributes(&self, collector: &mut AttributeCollector<'_>) {
        collector.push_opt_str(
            ACC_ATTR_INSTRUMENT_CONFIGURATION_REF,
            self.instrument_configuration_ref.as_deref(),
        );
        collector.push_opt_str(
            ACC_ATTR_EXTERNAL_SPECTRUM_ID,
            self.external_spectrum_id.as_deref(),
        );
        collector.push_opt_str(ACC_ATTR_SOURCE_FILE_REF, self.source_file_ref.as_deref());
        collector.push_opt_str(ACC_ATTR_SPECTRUM_REF, self.spectrum_ref.as_deref());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ion::attr_meta::CV_REF_ATTR;

    #[test]
    fn format_accession_produces_correct_string() {
        use crate::ion::attr_meta::AccessionTail;
        let tail = AccessionTail::from_raw(1_000_511);
        let accession = format_accession(tail);
        assert!(accession.ends_with(":1000511"));
        assert!(accession.starts_with(CV_REF_ATTR));
    }

    #[test]
    fn emit_spectrum_list_emits_count_and_ddpr() {
        use crate::mzml::structs::SpectrumList;
        let sl = SpectrumList {
            count: Some(3),
            default_data_processing_ref: Some("dp1".to_string()),
            spectra: vec![],
        };
        let out = assign_attributes(&sl, TagId::SpectrumList, 1, 0);
        assert_eq!(out.len(), 2);
        let tails: Vec<u32> = out
            .iter()
            .filter_map(|m| {
                m.accession
                    .as_deref()
                    .and_then(|a| a.rsplit_once(':').and_then(|(_, t)| t.parse::<u32>().ok()))
            })
            .collect();
        assert!(tails.contains(&ACC_ATTR_COUNT.raw()));
        assert!(tails.contains(&ACC_ATTR_DEFAULT_DATA_PROCESSING_REF.raw()));
    }

    #[test]
    fn emit_spectrum_skips_none_fields() {
        use crate::mzml::structs::Spectrum;
        let s = Spectrum {
            id: "scan=1".to_string(),
            index: Some(0),
            ..Default::default()
        };
        let out = assign_attributes(&s, TagId::Spectrum, 1, 0);
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn emit_attributes_all_emit_only_attr_prefix() {
        use crate::mzml::structs::Spectrum;
        let s = Spectrum {
            id: "x".to_string(),
            index: Some(0),
            native_id: Some("n".to_string()),
            default_array_length: Some(5),
            data_processing_ref: Some("dp".to_string()),
            source_file_ref: Some("sf".to_string()),
            spot_id: Some("sp".to_string()),
            ..Default::default()
        };
        let out = assign_attributes(&s, TagId::Spectrum, 1, 0);
        for m in &out {
            if let Some(acc) = &m.accession {
                assert!(
                    acc.starts_with(CV_REF_ATTR),
                    "unexpected accession prefix: {acc}"
                );
            }
        }
    }
}
