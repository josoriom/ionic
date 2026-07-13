use std::hint::black_box;
use std::path::PathBuf;

use criterion::{Criterion, criterion_group, criterion_main};
use ionic::ion::decoder::decode::ArrayAddress;
use ionic::{IonReader, MzML, Range, ReadOptions, WriteOptions, write_mzml_to_ion};

fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("data/ion/small.pwiz.1.1.ion")
}

fn open_reader() -> IonReader {
    IonReader::open_file(&fixture_path(), ReadOptions::default()).unwrap()
}

fn decode_run_to_model(c: &mut Criterion) {
    let mut reader = open_reader();
    c.bench_function("decode_run_to_model", |b| {
        b.iter(|| black_box(reader.to_mzml().unwrap()));
    });
}

fn decode_spectrum_values(c: &mut Criterion) {
    let mut reader = open_reader();
    let spectrum_count = reader.spectrum_count() as usize;
    let addresses: Vec<Vec<ArrayAddress>> = (0..spectrum_count)
        .filter_map(|index| reader.spectrum_array_addresses(index))
        .collect();
    c.bench_function("decode_spectrum_values", |b| {
        b.iter(|| {
            let mut values = Vec::new();
            for group in &addresses {
                for address in group {
                    reader.read_spectrum_values(address, &mut values).unwrap();
                    black_box(&values);
                }
            }
        });
    });
}

fn read_window_full(c: &mut Criterion) {
    let mut reader = open_reader();
    let spectrum_count = reader.spectrum_count() as usize;
    c.bench_function("read_window_full", |b| {
        b.iter(|| {
            for index in 0..spectrum_count {
                black_box(
                    reader
                        .read_window(index, Range { from: 0.0, to: f64::MAX })
                        .unwrap(),
                );
            }
        });
    });
}

fn encode_model_to_ion(c: &mut Criterion) {
    let model: MzML = {
        let mut reader = open_reader();
        reader.to_mzml().unwrap()
    };
    let mut group = c.benchmark_group("encode");
    group.sample_size(20);
    group.bench_function("encode_model_to_ion", |b| {
        b.iter(|| {
            let mut out = Vec::new();
            write_mzml_to_ion(&model, WriteOptions::default(), &mut out).unwrap();
            black_box(out);
        });
    });
    group.finish();
}

criterion_group!(
    benches,
    decode_run_to_model,
    decode_spectrum_values,
    read_window_full,
    encode_model_to_ion
);
criterion_main!(benches);
