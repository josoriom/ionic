#[macro_use]
mod common;

use common::test_files;

roundtrip_xml!(legacy_099_9_pwiz, test_files::legacy_pwiz_099_9);
roundtrip_xml!(legacy_099_10_pwiz, test_files::legacy_pwiz_099_10);
roundtrip_xml!(legacy_099_9_msdata, test_files::legacy_msdata_099_9);
roundtrip_xml!(legacy_099_10_msdata, test_files::legacy_msdata_099_10);
roundtrip_xml!(legacy_099_0_tiny1, test_files::legacy_tiny1_099_0);
roundtrip_xml!(legacy_099_0_tiny2_srm, test_files::legacy_tiny2_srm_099_0);
roundtrip_xml!(legacy_099_1_tiny2_srm, test_files::legacy_tiny2_srm_099_1);
roundtrip_xml!(
    legacy_099_0_tiny4_ltq_ft,
    test_files::legacy_tiny4_ltq_ft_099_0
);

roundtrip_ion!(
    legacy_099_9_pwiz_ion,
    test_files::legacy_pwiz_099_9,
    level = 9
);
roundtrip_ion!(
    legacy_099_10_pwiz_ion,
    test_files::legacy_pwiz_099_10,
    level = 9
);
roundtrip_ion!(
    legacy_099_9_msdata_ion,
    test_files::legacy_msdata_099_9,
    level = 9
);
roundtrip_ion!(
    legacy_099_10_msdata_ion,
    test_files::legacy_msdata_099_10,
    level = 9
);

#[test]
fn legacy_all_internal_test_files_parse_smoke() {
    for rel in common::test_files::INTERNAL_MZML_TEST_FILES {
        let mzml = common::parse_test_file(rel);
        let has_spectra = common::spectra(&mzml).len();
        let _ = has_spectra;
    }
}
