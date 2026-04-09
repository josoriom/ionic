mod common;

use common::assertions::*;
use common::test_files;

/// Checks that reading the same spectrum or chromatogram many times always gives the exact same result,
/// so access is stable and does not change data.
#[test]
fn repeated_spectrum_access_is_stable() {
    let mzml = test_files::tiny_pwiz_11();
    let ctx = SemanticContext::new(mzml);
    let sl = mzml
        .run
        .spectrum_list
        .as_ref()
        .expect("spectrumList parsed");
    let baseline = sl.spectra[0].clone();
    for i in 0..200 {
        let current = &sl.spectra[0];
        assert_spectrum_semantic_eq(
            &ctx,
            &ctx,
            &baseline,
            current,
            sl.default_data_processing_ref.as_deref(),
            sl.default_data_processing_ref.as_deref(),
            &format!("repeated spectrum read iter {i}"),
        );
    }
}

#[test]
fn repeated_chromatogram_access_is_stable() {
    let mzml = test_files::tiny_pwiz_11();
    let ctx = SemanticContext::new(mzml);
    let cl = mzml
        .run
        .chromatogram_list
        .as_ref()
        .expect("chromatogramList parsed");
    let baseline = cl.chromatograms[0].clone();
    for i in 0..200 {
        let current = &cl.chromatograms[0];
        assert_chromatogram_semantic_eq(
            &ctx,
            &ctx,
            &baseline,
            current,
            cl.default_data_processing_ref.as_deref(),
            cl.default_data_processing_ref.as_deref(),
            &format!("repeated chromatogram read iter {i}"),
        );
    }
}
