mod common;
use common::*;

#[test]
fn benchmark_decode_help_present() {
    let output = ionic().arg("convert").arg("--help").output().unwrap();

    assert!(output.status.success());
    let stdout = strip_ansi(&String::from_utf8_lossy(&output.stdout));
    assert!(
        stdout.contains("Benchmark decode speed"),
        "--help should describe --benchmark-decode, got: {stdout}"
    );
}
