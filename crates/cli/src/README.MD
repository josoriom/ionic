# CLI — Commands & Flags (with examples)

This CLI has **two subcommands**:

- `convert` — batch convert folders (recursive)
- `cat` — print JSON metadata for a single file (`.mzML`, `.ion`, or `.b32`)

Examples below use:

- `tiny.msdata.mzML0.99.9.mzML`
- `tiny.msdata.mzML0.99.9.ion`

---

## 1) `ionic cat`

Prints JSON to stdout for a **single file**.

### Flag

- `--file-path <PATH>`
  - Path to the file you want to inspect.
  - Supported file types: `.ion`, `.b32`.

### Example:

```bash
ionic cat --file-path crates/parser/data/ion/tiny.msdata.mzML0.99.9.ion
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
  - Default: `12`
  - Compression level (only relevant for mzML → ion/b32)

- `--overwrite`
  - If set, overwrites output files even if they already exist.
  - If not set, an output is skipped only if it already exists and looks complete.

- `--cores <N>`
  - Default: `1`
  - Number of worker threads used for conversion (Rayon thread pool).

### Conversion mode (optional)

If you don’t pass any mode flag, the default is `--mzml-to-ion`.

- `--mzml-to-ion`
  - Converts `.mzML` → `.ion`
  - Uses `encode(mzml, level, f32_compress=false)`

- `--mzml-to-b32`
  - Converts `.mzML` → `.b32`
  - Uses `encode(mzml, level, f32_compress=true)`

- `--ion-to-mzml`
  - Converts `.ion` and `.b32` → `.mzML`
  - Uses `decode(...)` into `MzML`, then `bin_to_mzml(&mzml)` to produce mzML XML

### Filtering flags (optional)

You may pass **at most one** of the following:

- `--pattern <STR>`
  - Unicode case-insensitive substring match.

- `--pattern-exact <STR>`
  - Case-sensitive substring match.

- `--regex <REGEX>`
  - Regex match against the filename.

---

### 2.1) Convert `.mzML` → `.ion/.b32`

Defaults:

- `--level 12`

Run (defaults):

```bash
ionic convert -i crates/parser/data/mzml -o crates/parser/data/ion
```

or

```bash
ionic convert --mzml-to-ion --input-path crates/parser/data/mzml --output-path crates/parser/data/ion
```

Custom compression level:

```bash
ionic convert --mzml-to-ion --input-path crates/parser/data/mzml --output-path crates/parser/data/ion --level 12
```

---

### 2.2) Convert `.ion/.b32` → `.mzML`

### Example:

```bash
ionic convert --ion-to-mzml --input-path crates/parser/data/ion --output-path crates/parser/data/mzml_out
```

(If the input tree contains `.b32` too, it is also converted by this mode.)

---
