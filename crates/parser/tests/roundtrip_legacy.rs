#[macro_use]
mod common;

use common::fixtures;

// XML roundtrips for legacy fixtures
roundtrip_xml!(legacy_099_9_pwiz, fixtures::legacy_pwiz_099_9);
roundtrip_xml!(legacy_099_10_pwiz, fixtures::legacy_pwiz_099_10);
roundtrip_xml!(legacy_099_9_msdata, fixtures::legacy_msdata_099_9);
roundtrip_xml!(legacy_099_10_msdata, fixtures::legacy_msdata_099_10);
roundtrip_xml!(legacy_099_0_tiny1, fixtures::legacy_tiny1_099_0);
roundtrip_xml!(legacy_099_0_tiny2_srm, fixtures::legacy_tiny2_srm_099_0);
roundtrip_xml!(legacy_099_1_tiny2_srm, fixtures::legacy_tiny2_srm_099_1);
roundtrip_xml!(
    legacy_099_0_tiny4_ltq_ft,
    fixtures::legacy_tiny4_ltq_ft_099_0
);

// Ion roundtrips for legacy fixtures
roundtrip_ion!(
    legacy_099_9_pwiz_ion,
    fixtures::legacy_pwiz_099_9,
    level = 9
);
roundtrip_ion!(
    legacy_099_10_pwiz_ion,
    fixtures::legacy_pwiz_099_10,
    level = 9
);
roundtrip_ion!(
    legacy_099_9_msdata_ion,
    fixtures::legacy_msdata_099_9,
    level = 9
);
roundtrip_ion!(
    legacy_099_10_msdata_ion,
    fixtures::legacy_msdata_099_10,
    level = 9
);

#[test]
fn legacy_all_internal_fixtures_parse_smoke() {
    for rel in common::fixtures::INTERNAL_MZML_FIXTURES {
        let mzml = common::parse_fixture(rel);
        let has_spectra = common::spectra(&mzml).len();
        // Legacy fixtures may or may not have spectra, but parsing must not panic
        let _ = has_spectra;
    }
}
