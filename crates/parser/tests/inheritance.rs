mod common;

use common::assertions::*;
use common::builders::*;
use common::BinaryDataExt;
use ionic::mzml::structs::*;

use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine};

// Tests 23-27: Default data processing refs, scan settings/source file refs,
// instrument software ref, legacy spectrum description,
// binary data array external metadata referenceable param group.

#[test]
fn default_data_processing_refs_roundtrip_semantic() {
    let encoded = BASE64_STANDARD.encode(BinaryData::F64(vec![1.0, 2.0, 3.0]).to_le_bytes());
    let xml = format!(
        concat!(
            "<mzML>",
            "{cv_list}",
            "<fileDescription><fileContent/><sourceFileList count=\"0\"/></fileDescription>",
            "<dataProcessingList count=\"2\">",
            "<dataProcessing id=\"dp_default\"><processingMethod order=\"0\"></processingMethod></dataProcessing>",
            "<dataProcessing id=\"dp_override\"><processingMethod order=\"0\"></processingMethod></dataProcessing>",
            "</dataProcessingList>",
            "<run id=\"dp-fallback\">",
            "<spectrumList count=\"1\" defaultDataProcessingRef=\"dp_default\">",
            "<spectrum index=\"0\" id=\"scan=1\" defaultArrayLength=\"3\">",
            "<binaryDataArrayList count=\"2\">",
            "<binaryDataArray arrayLength=\"3\" encodedLength=\"{len}\">",
            "<cvParam cvRef=\"MS\" accession=\"MS:1000514\" name=\"m/z array\"/>",
            "<cvParam cvRef=\"MS\" accession=\"MS:1000523\" name=\"64-bit float\"/>",
            "<cvParam cvRef=\"MS\" accession=\"MS:1000576\" name=\"no compression\"/>",
            "<binary>{encoded}</binary></binaryDataArray>",
            "<binaryDataArray arrayLength=\"3\" encodedLength=\"{len}\" dataProcessingRef=\"dp_override\">",
            "<cvParam cvRef=\"MS\" accession=\"MS:1000515\" name=\"intensity array\"/>",
            "<cvParam cvRef=\"MS\" accession=\"MS:1000523\" name=\"64-bit float\"/>",
            "<cvParam cvRef=\"MS\" accession=\"MS:1000576\" name=\"no compression\"/>",
            "<binary>{encoded}</binary></binaryDataArray>",
            "</binaryDataArrayList></spectrum></spectrumList>",
            "<chromatogramList count=\"1\" defaultDataProcessingRef=\"dp_default\">",
            "<chromatogram index=\"0\" id=\"tic\" defaultArrayLength=\"3\">",
            "<binaryDataArrayList count=\"1\">",
            "<binaryDataArray arrayLength=\"3\" encodedLength=\"{len}\">",
            "<cvParam cvRef=\"MS\" accession=\"MS:1000595\" name=\"time array\"/>",
            "<cvParam cvRef=\"MS\" accession=\"MS:1000523\" name=\"64-bit float\"/>",
            "<cvParam cvRef=\"MS\" accession=\"MS:1000576\" name=\"no compression\"/>",
            "<binary>{encoded}</binary></binaryDataArray>",
            "</binaryDataArrayList></chromatogram></chromatogramList>",
            "</run></mzML>"
        ),
        cv_list = DEFAULT_CV_LIST_XML,
        len = encoded.len(),
        encoded = encoded,
    );

    let mzml = common::parse_xml(&xml);
    let spectrum_list = mzml
        .run
        .spectrum_list
        .as_ref()
        .expect("spectrumList parsed");
    let spectrum = &spectrum_list.spectra[0];
    let arrays = common::spectrum_arrays(spectrum);
    assert_eq!(
        spectrum_list.default_data_processing_ref.as_deref(),
        Some("dp_default")
    );
    assert_eq!(arrays[0].data_processing_ref, None);
    assert_eq!(
        arrays[1].data_processing_ref.as_deref(),
        Some("dp_override")
    );

    assert_semantic_roundtrip_via_xml(&mzml, "dp-fallback-xml");
    assert_semantic_roundtrip_via_ion(&mzml, 9, "dp-fallback-ion");
}

#[test]
fn scan_settings_and_source_file_refs_roundtrip_semantic() {
    let xml = format!(
        r#"
<mzML>
  {cv_list}
  <fileDescription>
    <fileContent/>
    <sourceFileList count="1">
      <sourceFile id="SF1" name="input.raw" location="file:///tmp/input.raw"/>
    </sourceFileList>
  </fileDescription>
  <scanSettingsList count="1">
    <scanSettings id="SS1" instrumentConfigurationRef="IC1">
      <sourceFileRefList count="1">
        <sourceFileRef ref="SF1"/>
      </sourceFileRefList>
      <targetList count="1">
        <target>
          <userParam name="active time" type="seconds" value="0.5"/>
        </target>
      </targetList>
    </scanSettings>
  </scanSettingsList>
  <instrumentConfigurationList count="1">
    <instrumentConfiguration id="IC1" scanSettingsRef="SS1"/>
  </instrumentConfigurationList>
  <run id="scan-settings" defaultInstrumentConfigurationRef="IC1" defaultSourceFileRef="SF1">
    <sourceFileRefList count="1">
      <sourceFileRef ref="SF1"/>
    </sourceFileRefList>
    <spectrumList count="1">
      <spectrum index="0" id="scan=1" defaultArrayLength="0">
        <scanList count="1">
          <scan instrumentConfigurationRef="IC1"/>
        </scanList>
      </spectrum>
    </spectrumList>
    <chromatogramList count="0"/>
  </run>
</mzML>
"#,
        cv_list = DEFAULT_CV_LIST_XML
    );

    let mzml = common::parse_xml(&xml);
    assert_eq!(mzml.run.default_source_file_ref.as_deref(), Some("SF1"));
    assert_eq!(
        mzml.instrument_list
            .as_ref()
            .expect("instrument list parsed")
            .instrument[0]
            .scan_settings_ref
            .as_ref()
            .map(|value| value.r#ref.as_str()),
        Some("SS1")
    );

    assert_semantic_roundtrip_via_xml(&mzml, "scan-settings-source-files-xml");
    assert_semantic_roundtrip_via_ion(&mzml, 9, "scan-settings-source-files-ion");
}

#[test]
fn instrument_software_ref_roundtrip_semantic() {
    let xml = format!(
        r#"
<mzML>
  {cv_list}
  <fileDescription>
    <fileContent/>
    <sourceFileList count="0"/>
  </fileDescription>
  <softwareList count="2">
    <software id="legacy-sw" version="0.1">
      <cvParam cvRef="MS" accession="MS:1000531" name="software" value=""/>
    </software>
    <software id="acq-sw" version="1.0">
      <cvParam cvRef="MS" accession="MS:1000531" name="software" value=""/>
    </software>
  </softwareList>
  <instrumentConfigurationList count="1">
    <instrumentConfiguration id="IC1" softwareRef="legacy-sw">
      <softwareRef ref="acq-sw"/>
    </instrumentConfiguration>
  </instrumentConfigurationList>
  <run id="instrument-software-ref" defaultInstrumentConfigurationRef="IC1">
    <spectrumList count="1">
      <spectrum index="0" id="scan=1" defaultArrayLength="0">
        <scanList count="1">
          <scan/>
        </scanList>
      </spectrum>
    </spectrumList>
    <chromatogramList count="0"/>
  </run>
</mzML>
"#,
        cv_list = DEFAULT_CV_LIST_XML
    );

    let mzml = common::parse_xml(&xml);
    assert_eq!(
        mzml.instrument_list
            .as_ref()
            .expect("instrument list parsed")
            .instrument[0]
            .software_ref
            .as_ref()
            .map(|value| value.r#ref.as_str()),
        Some("acq-sw")
    );

    assert_semantic_roundtrip_via_xml(&mzml, "instrument-software-ref-xml");
    assert_semantic_roundtrip_via_ion(&mzml, 9, "instrument-software-ref-ion");
    assert_semantic_roundtrip_full_pipeline(&mzml, 9, "instrument-software-ref-full");
}

#[test]
fn legacy_spectrum_description_roundtrip_semantic() {
    let xml = format!(
        r#"
<mzML>
  {cv_list}
  <fileDescription>
    <fileContent/>
    <sourceFileList count="0"/>
  </fileDescription>
  <run id="legacy-spectrum-description">
    <spectrumList count="2">
      <spectrum index="0" id="S0" defaultArrayLength="0">
        <spectrumDescription>
          <scanList count="1"><scan/></scanList>
        </spectrumDescription>
      </spectrum>
      <spectrum index="1" id="S1" defaultArrayLength="0">
        <spectrumDescription>
          <scanList count="1"><scan/></scanList>
          <precursorList count="1">
            <precursor spectrumRef="S0">
              <selectedIonList count="1">
                <selectedIon>
                  <cvParam cvRef="MS" accession="MS:1000744" name="selected ion m/z" value="445.34"/>
                </selectedIon>
              </selectedIonList>
            </precursor>
          </precursorList>
          <productList count="1">
            <product>
              <isolationWindow>
                <cvParam cvRef="MS" accession="MS:1000827" name="isolation window target m/z" value="100.0"/>
              </isolationWindow>
            </product>
          </productList>
        </spectrumDescription>
      </spectrum>
    </spectrumList>
    <chromatogramList count="0"/>
  </run>
</mzML>
"#,
        cv_list = DEFAULT_CV_LIST_XML
    );

    let mzml = common::parse_xml(&xml);
    assert_eq!(common::spectra(&mzml).len(), 2);
    assert!(common::spectrum_by_id(&mzml, "S1")
        .spectrum_description
        .is_some());

    assert_semantic_roundtrip_via_xml(&mzml, "legacy-spectrum-description-xml");
    assert_semantic_roundtrip_via_ion(&mzml, 9, "legacy-spectrum-description-ion");
}

#[test]
fn binary_data_array_external_metadata_referenceable_param_group() {
    let xml = r#"
<mzML>
  <fileDescription>
    <fileContent/>
    <sourceFileList count="0"/>
  </fileDescription>
  <referenceableParamGroupList count="1">
    <referenceableParamGroup id="mz_params">
      <cvParam cvRef="MS" accession="MS:1000514" name="m/z array"/>
      <cvParam cvRef="MS" accession="MS:1000523" name="64-bit float"/>
      <cvParam cvRef="MS" accession="MS:1000576" name="no compression"/>
    </referenceableParamGroup>
  </referenceableParamGroupList>
  <run id="external-metadata-test">
    <spectrumList count="1">
      <spectrum index="0" id="scan=1" defaultArrayLength="15">
        <binaryDataArrayList count="1">
          <binaryDataArray encodedLength="160" arrayLength="15">
            <referenceableParamGroupRef ref="mz_params"/>
            <binary>AAAAAAAAAAAAAAAAAADwPwAAAAAAAABAAAAAAAAACEAAAAAAAAAQQAAAAAAAABRAAAAAAAAAGEAAAAAAAAAcQAAAAAAAACBAAAAAAAAAIkAAAAAAAAAkQAAAAAAAACZAAAAAAAAAKEAAAAAAAAAqQAAAAAAAACxA</binary>
          </binaryDataArray>
        </binaryDataArrayList>
      </spectrum>
    </spectrumList>
  </run>
</mzML>
"#;

    let mzml = common::parse_xml(xml);
    let s = common::spectrum_by_id(&mzml, "scan=1");
    let bdal = s
        .binary_data_array_list
        .as_ref()
        .expect("binaryDataArrayList parsed");
    assert_eq!(bdal.binary_data_arrays.len(), 1);

    let bda = &bdal.binary_data_arrays[0];
    assert_eq!(bda.referenceable_param_group_refs.len(), 1);
    assert_eq!(bda.referenceable_param_group_refs[0].r#ref, "mz_params");
    assert_eq!(bda.numeric_type, Some(NumericType::Float64));

    let values = bda
        .binary
        .as_ref()
        .expect("decoded binary payload present")
        .to_f64_vec();
    assert_eq!(values.len(), 15);
    for (i, v) in values.iter().enumerate() {
        rel_close_f64(
            *v,
            i as f64,
            EPS_REL_F64,
            &format!("external metadata bda[{i}]"),
        );
    }
}
