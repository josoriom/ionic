// ── pwiz fixtures ──────────────────────────────────────────────────────

fixture!(tiny_pwiz_10, "pwiz/example_data/tiny.pwiz.1.0.mzML");
fixture!(tiny_pwiz_11, "pwiz/example_data/tiny.pwiz.1.1.mzML");
fixture!(tiny_pwiz_111, "pwiz/example_data/tiny.pwiz.1.1.1.mzML");
fixture!(tiny2_pwiz_10, "pwiz/example_data/tiny2.pwiz.1.0.mzML");
fixture!(small_pwiz_10, "pwiz/example_data/small.pwiz.1.0.mzML");
fixture!(small_pwiz_11, "pwiz/example_data/small.pwiz.1.1.mzML");
fixture!(
    small_zlib_pwiz_11,
    "pwiz/example_data/small_zlib.pwiz.1.1.mzML"
);
fixture!(
    small_miape_pwiz_11,
    "pwiz/example_data/small_miape.pwiz.1.1.mzML"
);

// ── Internal parser fixtures ───────────────────────────────────────────

fixture!(anpc_test_mzml, "crates/parser/data/mzml/test.mzML");
fixture!(
    legacy_pwiz_099_10,
    "crates/parser/data/mzml/tiny.pwiz.mzML0.99.10.mzML"
);
fixture!(
    legacy_pwiz_099_9,
    "crates/parser/data/mzml/tiny.pwiz.mzML0.99.9.mzML"
);
fixture!(
    legacy_msdata_099_10,
    "crates/parser/data/mzml/tiny.msdata.mzML0.99.10.mzML"
);
fixture!(
    legacy_msdata_099_9,
    "crates/parser/data/mzml/tiny.msdata.mzML0.99.9.mzML"
);
fixture!(
    legacy_tiny1_099_0,
    "crates/parser/data/mzml/tiny1.mzML0.99.0.mzML"
);
fixture!(
    legacy_tiny2_srm_099_0,
    "crates/parser/data/mzml/tiny2_SRM.mzML0.99.0.mzML"
);
fixture!(
    legacy_tiny2_srm_099_1,
    "crates/parser/data/mzml/tiny2_SRM.mzML0.99.1.mzML"
);
fixture!(
    legacy_tiny4_ltq_ft_099_0,
    "crates/parser/data/mzml/tiny4_LTQ-FT.mzML0.99.0.mzML"
);

/// All pwiz fixture paths for iteration.
pub const PWIZ_FIXTURES: &[&str] = &[
    "pwiz/example_data/tiny.pwiz.1.0.mzML",
    "pwiz/example_data/tiny.pwiz.1.1.mzML",
    "pwiz/example_data/tiny.pwiz.1.1.1.mzML",
    "pwiz/example_data/tiny2.pwiz.1.0.mzML",
    "pwiz/example_data/small.pwiz.1.0.mzML",
    "pwiz/example_data/small.pwiz.1.1.mzML",
    "pwiz/example_data/small_zlib.pwiz.1.1.mzML",
    "pwiz/example_data/small_miape.pwiz.1.1.mzML",
];

/// All internal mzML fixture paths for iteration.
pub const INTERNAL_MZML_FIXTURES: &[&str] = &[
    "crates/parser/data/mzml/test.mzML",
    "crates/parser/data/mzml/tiny.pwiz.mzML0.99.10.mzML",
    "crates/parser/data/mzml/tiny.pwiz.mzML0.99.9.mzML",
    "crates/parser/data/mzml/tiny.msdata.mzML0.99.10.mzML",
    "crates/parser/data/mzml/tiny.msdata.mzML0.99.9.mzML",
    "crates/parser/data/mzml/tiny1.mzML0.99.0.mzML",
    "crates/parser/data/mzml/tiny2_SRM.mzML0.99.0.mzML",
    "crates/parser/data/mzml/tiny2_SRM.mzML0.99.1.mzML",
    "crates/parser/data/mzml/tiny4_LTQ-FT.mzML0.99.0.mzML",
];

/// All fixture paths combined.
pub const ALL_FIXTURES: &[&str] = &[
    "pwiz/example_data/tiny.pwiz.1.0.mzML",
    "pwiz/example_data/tiny.pwiz.1.1.mzML",
    "pwiz/example_data/tiny.pwiz.1.1.1.mzML",
    "pwiz/example_data/tiny2.pwiz.1.0.mzML",
    "pwiz/example_data/small.pwiz.1.0.mzML",
    "pwiz/example_data/small.pwiz.1.1.mzML",
    "pwiz/example_data/small_zlib.pwiz.1.1.mzML",
    "pwiz/example_data/small_miape.pwiz.1.1.mzML",
    "crates/parser/data/mzml/test.mzML",
    "crates/parser/data/mzml/tiny.pwiz.mzML0.99.10.mzML",
    "crates/parser/data/mzml/tiny.pwiz.mzML0.99.9.mzML",
    "crates/parser/data/mzml/tiny.msdata.mzML0.99.10.mzML",
    "crates/parser/data/mzml/tiny.msdata.mzML0.99.9.mzML",
    "crates/parser/data/mzml/tiny1.mzML0.99.0.mzML",
    "crates/parser/data/mzml/tiny2_SRM.mzML0.99.0.mzML",
    "crates/parser/data/mzml/tiny2_SRM.mzML0.99.1.mzML",
    "crates/parser/data/mzml/tiny4_LTQ-FT.mzML0.99.0.mzML",
];
