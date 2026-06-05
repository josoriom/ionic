mod common;

use std::fs;

use common::{canonical_diff_paths, decode_ion};
use ionic::{
    encoder::IonWriter,
    ion::{
        Ion,
        decoder::decode::DecoderConfig,
        encoder::{
            encode::{EncodingConfig, TARGET_BLOCK_UNCOMPRESSED_BYTES},
            ion_writer::stream_to_ion,
            utilities::SectionChunkMode,
        },
    },
    mzml::{MzmlReader, parse_mzml::parse_mzml},
};

fn config() -> EncodingConfig {
    EncodingConfig {
        compression_level: 0,
        force_f32: false,
        uncompressed_block_size: TARGET_BLOCK_UNCOMPRESSED_BYTES,
        parallel: false,
        section_chunk: SectionChunkMode::Memory,
    }
}

fn coordinate_xml() -> &'static [u8] {
    br#"
<mzML>
  <run id="coords">
    <spectrumList count="1">
      <spectrum index="0" id="scan=1" defaultArrayLength="0">
        <scanList count="1">
          <scan>
            <cvParam cvRef="IMS" accession="IMS:1000050" name="position x" value="11"/>
            <cvParam cvRef="IMS" accession="IMS:1000051" name="position y" value="22"/>
            <cvParam cvRef="IMS" accession="IMS:1000052" name="position z" value="3"/>
          </scan>
        </scanList>
      </spectrum>
    </spectrumList>
  </run>
</mzML>
"#
}

fn mixed_xml() -> &'static [u8] {
    br#"
<mzML>
  <run id="mixed">
    <spectrumList count="1" defaultDataProcessingRef="dp">
      <spectrum index="0" id="scan=1" defaultArrayLength="0"/>
    </spectrumList>
    <chromatogramList count="1" defaultDataProcessingRef="dp">
      <chromatogram index="0" id="tic" defaultArrayLength="0"/>
    </chromatogramList>
  </run>
</mzML>
"#
}

#[test]
fn stream_writer_roundtrips_like_mzml_writer() {
    let mzml = parse_mzml(coordinate_xml()).unwrap();
    let mut reader = MzmlReader::from_mzml(mzml.clone());
    let mut bytes = Vec::new();
    let mut writer = IonWriter::begin(&mut bytes, config()).unwrap();
    stream_to_ion(&mut reader, &mut writer).unwrap();
    let decoded = decode_ion(&bytes).unwrap();
    let diffs = canonical_diff_paths(&mzml, &decoded);
    assert!(diffs.is_empty(), "{diffs:#?}");
}

#[test]
fn spectrum_summary_keeps_coordinates() {
    let mzml = parse_mzml(coordinate_xml()).unwrap();
    let mut reader = MzmlReader::from_mzml(mzml);
    let mut bytes = Vec::new();
    let mut writer = IonWriter::begin(&mut bytes, config()).unwrap();
    stream_to_ion(&mut reader, &mut writer).unwrap();
    let ion = Ion::open(&bytes, DecoderConfig::default()).unwrap();
    let summary = ion.spec_summary(0).unwrap();
    assert_eq!(summary.position_x, 11);
    assert_eq!(summary.position_y, 22);
    assert_eq!(summary.position_z, 3);
}

#[test]
fn file_stream_reader_roundtrips_spectra_then_chromatograms() {
    let path =
        std::env::temp_dir().join(format!("ionic-stream-reader-{}.mzML", std::process::id()));
    fs::write(&path, mixed_xml()).unwrap();

    let mzml = parse_mzml(mixed_xml()).unwrap();
    let mut reader = MzmlReader::open(&path).unwrap();
    let mut bytes = Vec::new();
    let mut writer = IonWriter::begin(&mut bytes, config()).unwrap();
    stream_to_ion(&mut reader, &mut writer).unwrap();
    let decoded = decode_ion(&bytes).unwrap();
    let diffs = canonical_diff_paths(&mzml, &decoded);
    let _ = fs::remove_file(&path);
    assert!(diffs.is_empty(), "{diffs:#?}");
}
