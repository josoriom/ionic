# CLI — Commands & Flags (with examples)

This CLI has **two subcommands**:

- `convert` — batch convert folders (recursive)
- `cat` — print JSON metadata for a single file (`.mzML` or `.ion`)

Examples below use:

- `tiny.msdata.mzML0.99.9.mzML`
- `tiny.msdata.mzML0.99.9.ion`

---

## 1) `ionic cat`

Prints JSON to stdout for a **single file**.

### Argument & flags

- `<PATH>` (positional, required)
  - Path to the file you want to inspect. Supported: `.ion`, `.mzML`.
- `--check`
  - Verify every CRC section and report integrity instead of printing metadata.
- `-f, --full`
  - Print the full metadata (default prints a summary).
- `--scan <N>`
  - Print a single spectrum (1-based position in the spectrum list) as JSON.

### Example:

```bash
ionic cat crates/parser/data/ion/tiny.msdata.mzML0.99.9.ion
ionic cat --check crates/parser/data/ion/tiny.msdata.mzML0.99.9.ion
```

---

## 2) `ionic convert`

Batch converts files under a folder **recursively**, preserving the relative folder structure under `--output-path`.

### Common flags

- `-i, --input-path <DIR>`
  - Directory to scan recursively (required)

- `-o, --output-path <DIR>`
  - Output directory root (subfolders are created) (required)

- `--level <0..22>`
  - zstd compression level for the data blocks. `0` disables compression.

- `--mz-window <DA>`
  - Width, in Da (m/z units), of the windowed layout. A spectrum's m/z and intensity
    arrays are cut at every window boundary, and the same window across all spectra is
    routed to shared blocks, so an extracted-ion read opens only the blocks overlapping
    the queried m/z range. Smaller windows give finer random-access granularity; larger
    windows compress a little better.

- `--block-size <MB>`
  - Target uncompressed size of a data block before zstd. Larger blocks share more
    context (slightly better ratio on dense data) but make the read cache coarser.

- `--storage <memory|disk>`
  - Where section buffers are staged during encoding. `disk` bounds memory for large
    files; `memory` is faster for small ones.

- `--force-f32`
  - Store the f64 data arrays (m/z, intensity, time) as 32-bit floats — smaller output,
    lossy. Integer arrays are left unchanged.

- `--overwrite`
  - If set, overwrites output files even if they already exist.
  - If not set, an output is skipped only if it already exists and looks complete.

- `--cores <N>`
  - Default: `0` (use all available cores)
  - Number of worker threads used for conversion (Rayon thread pool). Pass `--cores 1`
    to keep the machine responsive during a large conversion.

### Conversion mode (optional)

If you don’t pass any mode flag, the default is `--mzml-to-ion`.

- `--mzml-to-ion`
  - Converts `.mzML` → `.ion` (add `--force-f32` to store f64 arrays as f32; smaller, lossy)

- `--ion-to-mzml`
  - Converts `.ion` → `.mzML`
  - Uses `decode(...)` into `MzML`, then `bin_to_mzml(&mzml)` to produce mzML XML

### Filtering flags (optional)

You may pass **at most one** of the following:

- `--pattern <STR>`
  - Unicode case-insensitive substring match.

- `--pattern-exact <STR>`
  - Case-sensitive substring match.

- `--regex <REGEX>`
  - Regex match against the filename.

### Migration flags (optional)

- `--update`
  - Re-encode existing `.ion` files in place into the current windowed format, reading
    each old file through the legacy compatibility decoder and verifying the result
    round-trips bit-exact before replacing the original (atomic temp + rename).
- `--dry-run`
  - With `--update`, report what would change without writing anything.

---

### 2.1) Convert `.mzML` → `.ion`

Defaults:

- `--level 22`, `--mz-window 50`, `--block-size 16`

Run (defaults):

```bash
ionic convert -i crates/parser/data/mzml -o crates/parser/data/ion
```

or

```bash
ionic convert --mzml-to-ion --input-path crates/parser/data/mzml --output-path crates/parser/data/ion
```

Custom compression level and m/z window:

```bash
ionic convert --mzml-to-ion -i crates/parser/data/mzml -o crates/parser/data/ion --level 22 --mz-window 100
```

---

### 2.2) Convert `.ion` → `.mzML`

### Example:

```bash
ionic convert --ion-to-mzml --input-path crates/parser/data/ion --output-path crates/parser/data/mzml_out
```

---
