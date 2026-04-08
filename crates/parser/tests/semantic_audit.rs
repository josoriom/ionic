mod common;

use common::assertions::*;
use common::helpers::*;

#[test]
fn ref_groups_and_list_metadata_survive_full_pipeline() {
    let xml = format!(
        r#"
<mzML>
  {cv_list}
  <referenceableParamGroupList count="8">
    <referenceableParamGroup id="fc-group"><userParam name="fc-note" value="file-content" type="xsd:string"/></referenceableParamGroup>
    <referenceableParamGroup id="sf-group"><userParam name="sf-note" value="source-file" type="xsd:string"/></referenceableParamGroup>
    <referenceableParamGroup id="contact-group"><userParam name="contact-note" value="contact" type="xsd:string"/></referenceableParamGroup>
    <referenceableParamGroup id="chrom-group"><userParam name="chrom-note" value="chrom" type="xsd:string"/></referenceableParamGroup>
    <referenceableParamGroup id="iw-group"><userParam name="iw-note" value="isolation-window" type="xsd:string"/></referenceableParamGroup>
    <referenceableParamGroup id="act-group"><userParam name="act-note" value="activation" type="xsd:string"/></referenceableParamGroup>
    <referenceableParamGroup id="si-group"><userParam name="si-note" value="selected-ion" type="xsd:string"/></referenceableParamGroup>
    <referenceableParamGroup id="target-group"><userParam name="target-note" value="target" type="xsd:string"/></referenceableParamGroup>
  </referenceableParamGroupList>
  <fileDescription>
    <fileContent>
      <referenceableParamGroupRef ref="fc-group"/>
    </fileContent>
    <sourceFileList count="1">
      <sourceFile id="SF1" name="input.raw" location="file:///tmp/input.raw">
        <referenceableParamGroupRef ref="sf-group"/>
      </sourceFile>
    </sourceFileList>
    <contact>
      <referenceableParamGroupRef ref="contact-group"/>
    </contact>
  </fileDescription>
  <scanSettingsList count="1">
    <scanSettings id="SS1">
      <targetList count="1">
        <target>
          <referenceableParamGroupRef ref="target-group"/>
        </target>
      </targetList>
    </scanSettings>
  </scanSettingsList>
  <run id="semantic-audit" defaultSourceFileRef="SF1">
    <sourceFileRefList count="1">
      <sourceFileRef ref="SF1"/>
    </sourceFileRefList>
    <spectrumList count="1">
      <spectrum index="0" id="scan=1" defaultArrayLength="0">
        <precursorList count="1">
          <userParam name="precursor-list-note" value="keep-me" type="xsd:string"/>
          <precursor spectrumRef="scan=1" sourceFileRef="SF1">
            <isolationWindow>
              <referenceableParamGroupRef ref="iw-group"/>
            </isolationWindow>
            <selectedIonList count="1">
              <selectedIon>
                <referenceableParamGroupRef ref="si-group"/>
              </selectedIon>
            </selectedIonList>
            <activation>
              <referenceableParamGroupRef ref="act-group"/>
            </activation>
          </precursor>
        </precursorList>
        <productList count="1">
          <userParam name="product-list-note" value="keep-me-too" type="xsd:string"/>
          <product sourceFileRef="SF1">
            <isolationWindow>
              <referenceableParamGroupRef ref="iw-group"/>
            </isolationWindow>
          </product>
        </productList>
      </spectrum>
    </spectrumList>
    <chromatogramList count="1">
      <chromatogram index="0" id="tic" defaultArrayLength="0">
        <referenceableParamGroupRef ref="chrom-group"/>
        <precursor spectrumRef="scan=1" sourceFileRef="SF1">
          <isolationWindow>
            <referenceableParamGroupRef ref="iw-group"/>
          </isolationWindow>
          <selectedIonList count="1">
            <selectedIon>
              <referenceableParamGroupRef ref="si-group"/>
            </selectedIon>
          </selectedIonList>
          <activation>
            <referenceableParamGroupRef ref="act-group"/>
          </activation>
        </precursor>
        <product sourceFileRef="SF1">
          <isolationWindow>
            <referenceableParamGroupRef ref="iw-group"/>
          </isolationWindow>
        </product>
      </chromatogram>
    </chromatogramList>
  </run>
</mzML>
"#,
        cv_list = DEFAULT_CV_LIST_XML
    );

    let mzml = common::parse_xml(&xml);
    assert_semantic_roundtrip_full_pipeline(&mzml, 9, "semantic-audit-full-pipeline");
}
