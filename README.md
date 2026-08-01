<img src="assets/ion-file-glyph.svg" alt="ionic" width="110" align="right">

# Ionic

[![CI](https://github.com/phenological/ionic/actions/workflows/rust-tests.yml/badge.svg)](https://github.com/phenological/ionic/actions/workflows/rust-tests.yml)

**A streamable binary file format for mass-spectrometry profiling and imaging data, and
the Rust library and CLI that read and write it.**

Ionic converts losslessly to and from mzML, depends on nothing but standard byte
operations, and compiles to WebAssembly, so the same reader runs in a server, a notebook
or a browser tab.

---

## Why

Metabolic phenotyping has reached a scale at which the volume of data, not the
sophistication of the model, limits what can be learned from it. Population-scale
mass-spectrometry cohorts are the substrate for machine learning, which must iterate over
the entire corpus, so the size and speed at which data can be read set the ceiling on what
is feasible. At the same time, untargeted studies nominate thousands of features whose
underlying peaks are almost never inspected, because with conventional formats that means
retrieving gigabytes and loading a heavyweight tool per feature.

The community exchange format for mass-spectrometry data (mzML), used in proteomics,
metabolic profiling and imaging, is text-based and thus too large for long-term storage,
and has to be read in full before any spectrum can be reached. More compact binary
alternatives such as mzMLb solve the size problem but are built on general-purpose storage
libraries such as HDF5, which tie a file to a particular software stack and cannot
reasonably be compiled to a lightweight target such as WebAssembly.

Ionic stores the same information as native numeric types in independently compressed
Zstandard blocks, addressed through small fixed-width directories. A reader searches an
index, finds where a spectrum lives, requests that byte range, and leaves the rest of the
file untouched and compressed. Over HTTP, that is a range request; on disk, it is a seek.

## What is in this repository

| Path | Contents |
| --- | --- |
| `crates/parser` | The `ionic` library: mzML parser and writer, `.ion` reader and writer |
| `crates/cli` | The `ionic` command-line tool (`convert`, `cat`) |
| `crates/xtask` | Version and release housekeeping (`config.json` ⇄ `Cargo.toml` ⇄ format constants) |
| `spec/` | The format specification: [overview](spec/README.MD), [byte layout v0](spec/v0.md) |
| `artifacts/` | Prebuilt CLI binaries per version and target |

Status: pre-release. Package `0.1.0`, `format_version = 0`. The reader accepts version 0
only and rejects any unknown codec or dtype, so a file written by a newer build fails
loudly rather than silently. The bytes may still change before the freeze.

## Install

### Command-line tool

Prebuilt binaries for macOS (arm64, x86_64), Linux (arm64, x86_64) and Windows (x86_64)
live under `artifacts/`. For example, on Apple silicon:

```bash
INSTALL_DIR="$HOME/.local/bin"          # or wherever you keep CLI programs
mkdir -p "$INSTALL_DIR"
curl -fL "https://raw.githubusercontent.com/phenological/ionic/main/artifacts/0.1.0/aarch64-apple-darwin/ionic" \
  -o "$INSTALL_DIR/ionic"
chmod +x "$INSTALL_DIR/ionic"
ionic --help
```

Full per-platform instructions, including adding the directory to `PATH`:
[crates/cli/README.MD](crates/cli/README.MD).

### Library

```bash
cargo add ionic --git https://github.com/phenological/ionic --branch main
```

## Command line

Two subcommands. Full reference: [crates/cli/src/README.MD](crates/cli/src/README.MD).

### `ionic convert`

Converts a file or a folder tree, recursively, preserving the relative structure under the
output root.

```bash
# mzML to ion (the default direction)
ionic convert -i data/mzml -o data/ion

# ion back to mzML
ionic convert --ion-to-mzml -i data/ion -o data/mzml_out

# re-encode existing .ion files in place, here with a narrower m/z window
ionic convert --update -i data/ion --mz-window 50

# tighter random-access granularity, lossy 32-bit arrays
ionic convert -i data/mzml -o data/ion --mz-window 50 --force-f32
```

`-i` takes a single file or a directory. `-o` is required except with `--update`, where
omitting it rewrites each file in place; writes go through a temporary file that is moved
over the target only on success, so an interrupted run leaves the original intact.

| Flag | Default | Meaning |
| --- | --- | --- |
| `--level <0..22>` | `22` | zstd level for data blocks (`0` disables compression) |
| `--mz-window <DA>` | `250` | Width of the m/z split, in Da. Smaller windows read less per query, larger ones compress slightly better |
| `--block-size <MB>` | `8` | Target uncompressed block size before zstd |
| `--storage <memory\|disk>` | `disk` | Where section tables are staged during encoding |
| `--force-f32` | off | Store f64 arrays as 32-bit floats (smaller, lossy) |
| `--cores <N>` | `0` (all) | Rayon worker threads |
| `--many-files` | off | Parallelise across files instead of inside each file |
| `--overwrite` | off | Rewrite outputs that already exist |
| `--pattern`, `--pattern-exact`, `--regex` | | Filter which input filenames are converted (pass at most one) |
| `--mzml-to-ion`, `--ion-to-mzml`, `--update` | `--mzml-to-ion` | Direction (pass at most one). `--update` re-encodes `.ion` to `.ion` with the current settings, without going back to the mzML source |

### `ionic cat`

Prints JSON for a single `.ion` or `.mzML` file.

```bash
ionic cat run.ion                 # summary
ionic cat -f run.ion              # full metadata
ionic cat --check run.ion         # verify every CRC and report integrity
ionic cat --scan 1 run.ion        # one spectrum's metadata (1-based)
ionic cat --scan-full 1 run.ion   # the same, plus its m/z and intensity arrays
ionic cat --chrom 1 run.ion       # likewise for chromatograms
ionic cat --chrom-full 1 run.ion
```

`--check`, `--scan`, `--scan-full`, `--chrom` and `--chrom-full` are mutually exclusive.

## Library

### Convert

```rust
use std::path::Path;

ionic::mzml_to_ion(Path::new("run.mzML"), Path::new("run.ion"))?;
ionic::ion_to_mzml(Path::new("run.ion"), Path::new("run.mzML"))?;
```

### Read whole arrays

```rust
use ionic::{ArrayKind, IonReader, ReadOptions};

let mut reader = IonReader::open_file(Path::new("run.ion"), ReadOptions::default())?;

println!("{} spectra", reader.spectrum_count());
let mz = reader.get_spectrum_array(0, ArrayKind::Mz)?;
let intensity = reader.get_spectrum_array(0, ArrayKind::Intensity)?;
```

`spectrum_summary(i)` and `spectrum_summaries()` return the fixed-width per-item records
(retention time, base peak, polarity, MS level, and a few more) without decoding a single
data block, which is what makes filtering cheap. `spectrum(i)` and `chromatogram(i)`
rebuild a full item, arrays included; `spectrum_metadata_at(i)` returns just its CV
parameters; `to_mzml()` rebuilds the whole run as an mzML model.

### Read only a window

The point of the format. A spectrum's arrays are cut at m/z window boundaries, and the
same window across all spectra is routed to shared blocks, so an extracted-ion read opens
only the blocks whose window overlaps the query.

```rust
use ionic::{IonReader, Range, ReadOptions, Select};

let mut reader = IonReader::open_file(Path::new("run.ion"), ReadOptions::default())?;

// one spectrum, one m/z range
let slice = reader.read_window(0, Range { from: 180.0, to: 182.0 })?;

// every spectrum in a retention-time range, at MS1, over the same m/z range
reader.scans_in(
    Range { from: 180.0, to: 182.0 },
    Select::Rt(Range { from: 2.0, to: 4.0 }),
    Some(1),
    &mut |window| {
        // window.index, window.summary.rt, window.mz, window.intensity
    },
)?;
```

For imaging data, swap the retention-time selector for a spatial one:
`Select::Area(Pixel { x, y, z })` filters on the `position_x/y/z` fields carried in the
per-item summary, so a region of interest is selected without decoding array data, exactly
as a retention-time range is.

### Read over the network

`byte_ranges` answers "which byte ranges would this query touch?" without reading them,
and `CallbackSource` lets you supply the bytes however you like, which is how a browser
turns a query into HTTP range requests.

```rust
use ionic::{ByteRange, CallbackSource, IonReader, Range, ReadOptions, open_ranges};
use std::sync::Arc;

// the ranges a reader must fetch before it can open the file at all
let ranges: Vec<ByteRange> = open_ranges(&first_1024_bytes)?;

let source = Arc::new(CallbackSource::new(|range: ByteRange| {
    fetch_http_range(range.offset, range.length)      // your transport
}));
let mut reader = IonReader::open_source(source, ReadOptions::default())?;

let needed = reader.byte_ranges(0, Range { from: 180.0, to: 182.0 })?;
```

### Write

```rust
use ionic::{FileWriter, IonWriter, MemoryReader, MzML, Spectrum, WriteOptions};

let mzml = MzML::from_spectra(vec![
    Spectrum::new("scan=1", vec![100.0, 200.0], vec![10.0, 20.0]),
]);
let mut source = MemoryReader::new(mzml);
let mut output = FileWriter::open_path(Path::new("out.ion"))?;
let mut writer = IonWriter::create(&mut output, WriteOptions::default())?;
writer.write_stream(&mut source)?;
output.flush()?;
```

`IonWriter::create` takes any `WriteBytes` sink, so writing into a `Vec<u8>` in memory
works the same way. Sources implement `ScanStream`, so a converter can stream scans from
anywhere without materialising the run.

### Options

```rust
pub struct ReadOptions {
    pub max_cached_bytes: usize,                 // decoded-block cache cap; default 256 MiB
    pub verify_checksums: bool,                  // check CRCs and layout on open; default true
    pub parallel: bool,                          // decode blocks in parallel; default true
    pub decompression_limit: DecompressionLimit, // zip-bomb guard; default 2 GiB
}

pub struct WriteOptions {
    pub compression_level: u8,           // 0 = off, 1..=22 zstd; default 22
    pub force_f32: bool,                 // narrow f64 arrays to f32 (lossy); default false
    pub block_size: usize,               // target uncompressed block bytes; default 1 MiB
    pub parallel: bool,                  // default true
    pub section_storage: SectionStorage, // Memory or Disk; default Memory
    pub mz_window: f64,                  // m/z window width for range reads; default 100.0
}
```

The CLI picks different defaults from the library (`--block-size 8` MB, `--mz-window 250`
Da, `--storage disk`), tuned for whole-folder conversions of large runs.

## How the format works

The full byte layout is in [`spec/v0.md`](spec/v0.md); the shape of the idea is in
[`spec/README.MD`](spec/README.MD). In brief:

A fixed 1024-byte header holds the global settings and, for every section, its byte offset
and length. The reader trusts the header, never adjacency, so sections can be laid out in
any order and the file can be written in one pass with the header patched in last.

| Section | Role |
| --- | --- |
| Array blocks | The heavy data: m/z, intensity and time packed as native types into independently compressed blocks, with a directory at the tail |
| `A0` / `B0` | Window directory: per m/z window, which spectra store data there and which array segments hold it. The cross-item index an extracted-ion read enters first. Pointers only |
| `A1` / `B1` | Fixed 80-byte per-item summary (retention time, base peak, polarity, MS level), so filtering decodes no array data |
| `A2` / `B2` | Per-item array index |
| `A3` / `B3` | Per-array-segment address: block, element offset and count, dtype, filter. The single source of truth for where data lives |
| `C` / `D` / `E` | Metadata in columns for spectra, chromatograms and the run as a whole |

`A*` covers spectra, `B*` chromatograms. Chromatograms are not split by m/z, so `B0` puts
every chromatogram in a single window.

Reading one whole item goes `A1 → A2 → A3`: filter on `A1` with nothing decoded, follow
`A2`/`A3` to the blocks that hold the arrays, decompress only those, then resolve the
item's parameters from the columns in `C`/`D`. Reading one m/z range across many items
goes `A0 → A1 → A3` instead: enter the window directory, take the entries for the windows
that touch the range, read `A1[spectrum_index]` for retention time, and follow the `A3`
rows those entries name. Since finite retention times are stored ascending, a window's
entries are rt-ascending too and an rt range is found by binary search.

Metadata is columnar and keyed by CV accession, never by human name: every parameter
becomes a row, all rows share the same columns, and one long array per field packs and
compresses far better than a crowd of small mixed objects. Structural attributes with no
CV term use Ionic's own vocabulary. `C` and `D` are split into fixed groups of items, each
compressed on its own, so reading one item's metadata decompresses one group rather than
the whole section.

`A1`/`B1` are a rebuildable cache: every field also lives in `C`/`D` under its accession.
That is why they are the one part of version 0 allowed to grow, through a reserved tail in
each record. Everything else is fixed, and changing it means a new `format_version`.

Array bytes are byte-shuffled (and delta-shuffled where it pays) before zstd, which is
where a large part of the compression ratio comes from. The transpose has SIMD backends
for x86_64, aarch64 and wasm32, with a scalar fallback, all cross-validated in CI.

## Integrity

The file opens with the signature `IONIC\0\0\0` and ends with the trailer `\0\0\0CINOI`,
and the header records the total file size, so a truncated or wrong file is caught
immediately. The header, every metadata section, every data block and every block
directory carries a CRC-32, so a reader can check a part before trusting it.
`ReadOptions::verify_checksums` (on by default) enforces this at open time,
`DecompressionLimit` caps what a malicious file can force you to allocate, and
`ionic cat --check` reports the whole picture from the command line.

Nothing fails quietly: `A0`/`B0` and `A3`/`B3` are required for range and extracted-ion
reads and there is no full-array fallback behind them, so a missing, CRC-failed or
malformed index is an error rather than a silently narrower answer.

## Portability

The library depends only on standard byte operations: no HDF5, no storage engine, nothing
to install alongside a file. On `wasm32-unknown-unknown` the build swaps `zstd` for the
pure-Rust `ruzstd` and drops `rayon` and `memmap2`, so the same reader compiles into a
browser bundle. That is what [ion-beam](https://github.com/phenological/ion-beam) runs on.

## Building and testing

```bash
make build      # cargo build --workspace
make test       # cargo test --workspace
make check      # verify config.json matches Cargo.toml and the format constants
make sync       # propagate the version in config.json into the workspace
make release    # cross-build CLI binaries into artifacts/ and write the manifest
```

CI runs the full test suite on Linux, macOS and Windows for both x86_64 and aarch64, plus
a `wasm-pack` run of the byte-transpose SIMD tests on `wasm32-unknown-unknown`, and checks
that `config.json`, `Cargo.toml` and the in-code format constants agree.

Test data lives in `crates/parser/data/`: mzML files spanning the 0.99.x drafts and 1.0/1.1
releases, and their `.ion` counterparts. The suite covers round-trips in both directions,
XSD compliance, malformed input, float edge cases, concurrency and byte-exact fingerprints.

## Related projects

- **[ion-beam](https://github.com/phenological/ion-beam)**: a browser viewer for `.ion`
  files that downloads only the bytes it needs, with an inspector showing exactly which
  regions of the file were fetched. [Try it live](https://phenological.github.io/ion-beam/).
- **[Quant·ion](https://github.com/phenological/quantion)**: the Rust processing toolkit
  (peak picking, baselines, noise, untargeted feature detection) with Python, R and
  JavaScript wrappers.
- **[ion-files](https://github.com/phenological/ion-files)**: a small public collection of
  demo `.ion` files.

## Citing

If you use Ionic, Quant·ion or ion-beam, please cite:

> Reading only what you need: a dependency-free, streamable format and cross-language
> toolkit for scalable LC-MS feature detection. Preprint, 2026.
> DOI: [10.XXXXX/XXXXXX](https://doi.org/10.XXXXX/XXXXXX)

```bibtex
@article{ionic2026,
  title   = {Reading only what you need: a dependency-free, streamable format
             and cross-language toolkit for scalable LC-MS feature detection},
  author  = {TBD},
  year    = {2026},
  journal = {TBD},
  doi     = {10.XXXXX/XXXXXX}
}
```

## License

MIT.
