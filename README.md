# Ionic

[![CI](https://github.com/josoriom/ionic/actions/workflows/rust-tests.yml/badge.svg)](https://github.com/josoriom/ionic/actions/workflows/rust-tests.yml)

[CLI](crates/cli/README.MD)

## Install

```bash
cargo add ionic --git https://github.com/josoriom/ionic --branch main
```

## Usage

### Convert .mzML to .ion

```rust
ionic::mzml_to_ion(Path::new("run.mzML"), Path::new("run.ion"))?;
```

### Convert .ion to .mzML

```rust
ionic::ion_to_mzml(Path::new("run.ion"), Path::new("run.mzML"))?;
```

### Read an .ion file

```rust
use ionic::{ArrayKind, IonReader, ReadOptions};

let mut reader = IonReader::open_file(Path::new("run.ion"), ReadOptions::default())?;
let mz = reader.get_spectrum_array(0, ArrayKind::Mz)?;
let intensity = reader.get_spectrum_array(0, ArrayKind::Intensity)?;
```

### Reading options — ReadOptions

```rust
pub struct ReadOptions {
    pub max_cached_bytes: usize,                 // decoded-block cache cap; default 256 MiB
    pub verify_checksums: bool,                  // check CRCs + layout on open; default true
    pub parallel: bool,                          // decode blocks in parallel; default true
    pub decompression_limit: DecompressionLimit, // zip-bomb guard; type from ionic::ion::
}
```

### Write an .ion file

Memory

```rust
use ionic::{IonWriter, MemoryReader, MzML, Spectrum, WriteOptions};

let mzml = MzML::from_spectra(vec![Spectrum::new("scan=1", vec![100.0, 200.0], vec![10.0, 20.0])]);
let mut source = MemoryReader::new(mzml);
let mut out: Vec<u8> = Vec::new();
let mut writer = IonWriter::create(&mut out, WriteOptions::default())?;
writer.write_stream(&mut source)?;
```

File path

```rust
use ionic::{FileWriter, IonWriter, MemoryReader, MzML, Spectrum, WriteOptions};

let mzml = MzML::from_spectra(vec![Spectrum::new("scan=1", vec![100.0, 200.0], vec![10.0, 20.0])]);
let mut source = MemoryReader::new(mzml);
let mut output = FileWriter::open_path(Path::new("out.ion"))?; // or FileWriter::open("out.ion")
let mut writer = IonWriter::create(&mut output, WriteOptions::default())?;
writer.write_stream(&mut source)?;
output.flush()?;
```

### WriteOptions

```rust
pub struct WriteOptions {
    pub compression_level: u8,           // 0 = off, 1..=22 zstd; default 22
    pub force_f32: bool,                 // narrow f64 arrays to f32 (lossy); default false
    pub block_size: usize,               // target uncompressed block bytes; default 1 MiB
    pub parallel: bool,                  // default true
    pub section_storage: SectionStorage, // Memory or Disk; default Memory
    pub mz_window: f64,                  // m/z window width for range-read indexing; default 100.0
}
```

- [Specs](spec/README.MD)
