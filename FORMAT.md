# IONIC format v1 (frozen baseline)

This is the on-disk contract for IONIC version 1. It describes the grouped-metadata
layout that is the current baseline. The reader and writer must match this document.
Any change to a used byte, a record size, or the meaning of a field is a format
change and is out of scope; the only allowed growth is the reserved tail of A0/B0
(see "Extensibility").

All integers are little-endian. The file is seekable; the header is written last.

## Design rules

- Source-independent: the format stores items, arrays, summaries, metadata, and
  offsets into its own file. It knows nothing about where the data came from.
- Metadata is keyed by PSI accession. Structural attributes with no PSI term use
  Ionic's reserved attribute namespace. The accession is the authority, never the
  human name.
- Offsets and lengths in the header are authoritative. Physical order is fixed but a
  reader must trust the header, not adjacency.

## Section map (letters used in the workplan)

| Letter | Role | Shape |
|---|---|---|
| header | file header | fixed 1024 bytes |
| — | spectrum array blocks (`packed_spectra`) | block container, directory at tail |
| — | chromatogram array blocks (`packed_chroms`) | block container, directory at tail |
| A0 | spectrum fast-filter summary (`spec_summary`) | fixed 128-byte record per spectrum |
| A1 | spectrum array index (`spec_entries`) | fixed 16-byte record per spectrum |
| A2 | spectrum array refs (`spec_arrayrefs`) | fixed 32-byte record per array |
| B0 | chromatogram fast-filter summary (`chrom_summary`) | fixed 128-byte record per chromatogram |
| B1 | chromatogram array index (`chrom_entries`) | fixed 16-byte record per chromatogram |
| B2 | chromatogram array refs (`chrom_arrayrefs`) | fixed 32-byte record per array |
| C | spectrum metadata (`spec_meta`) | grouped, directory at tail |
| D | chromatogram metadata (`chrom_meta`) | grouped, directory at tail |
| E | global metadata (`global_meta`) | one monolithic compressed section |

Physical order: header, packed_spectra, packed_chroms, A0, A1, A2, B0, B1, B2, C, D,
E, trailer. Every section starts on an 8-byte boundary.

## Header (1024 bytes)

| Offset | Size | Field |
|---|---|---|
| 0 | 8 | signature `IONIC\0\0\0` |
| 8 | 1 | endianness flag (0 = little-endian) |
| 9 | 2 | format version (u16, = 1) |
| 11 | 1 | compression codec (0 none, 1 zstd) |
| 12 | 1 | compression level (0–22) |
| 13 | 1 | default array filter (PackingId, see codes) |
| 14 | 2 | reserved, zero |
| 16 | 8 | target block uncompressed size |
| 24 | 8 | off A0 (spec_summary) |
| 32 | 8 | len A0 |
| 40 | 8 | off A1 (spec_entries) |
| 48 | 8 | len A1 |
| 56 | 8 | off A2 (spec_arrayrefs) |
| 64 | 8 | len A2 |
| 72 | 8 | off B0 (chrom_summary) |
| 80 | 8 | len B0 |
| 88 | 8 | off B1 (chrom_entries) |
| 96 | 8 | len B1 |
| 104 | 8 | off B2 (chrom_arrayrefs) |
| 112 | 8 | len B2 |
| 120 | 8 | off C (spec_meta) |
| 128 | 8 | len C |
| 136 | 8 | off D (chrom_meta) |
| 144 | 8 | len D |
| 152 | 8 | off E (global_meta) |
| 160 | 8 | len E |
| 168 | 8 | off packed_spectra |
| 176 | 8 | len packed_spectra |
| 184 | 8 | off packed_chroms |
| 192 | 8 | len packed_chroms |
| 200 | 8 | spectrum block count |
| 208 | 8 | chromatogram block count |
| 216 | 8 | spectrum count |
| 224 | 8 | chromatogram count |
| 232 | 8 | C row count |
| 240 | 8 | C numeric count |
| 248 | 8 | C string count |
| 256 | 8 | D row count |
| 264 | 8 | D numeric count |
| 272 | 8 | D string count |
| 280 | 8 | E row count |
| 288 | 8 | E numeric count |
| 296 | 8 | E string count |
| 304 | 8 | spectrum array type count |
| 312 | 8 | chromatogram array type count |
| 320 | 8 | C uncompressed size |
| 328 | 8 | D uncompressed size |
| 336 | 8 | E uncompressed size |
| 344 | 8 | total file size |
| 352 | 8 | metadata group size (items per group, = 8192) |
| 360 | 8 | C group count |
| 368 | 8 | D group count |
| 376 | 632 | reserved, zero (checked; off-limits) |
| 1008 | 4 | C crc32 |
| 1012 | 4 | D crc32 |
| 1016 | 4 | E crc32 |
| 1020 | 4 | header crc32 (over bytes 0..1020) |

## A0 — spectrum fast-filter summary (128 bytes)

| Offset | Size | Field | Type / sentinel | State |
|---|---|---|---|---|
| 0 | 8 | rt_seconds | f64 | used |
| 8 | 8 | base_peak_mz | f64 | used |
| 16 | 8 | selected_ion_mz | f64 | used |
| 24 | 8 | base_peak_int | f64 | used |
| 32 | 8 | total_ion_current | f64 | used |
| 40 | 1 | ms_level | u8 | used |
| 41 | 1 | polarity | u8 (0 unknown, 1 positive, 2 negative) | used |
| 42 | 4 | position_x | u32, 0 = unknown | reserved (Phase 5) |
| 46 | 4 | position_y | u32, 0 = unknown | reserved (Phase 5) |
| 50 | 4 | position_z | u32, 0 = unknown | reserved (Phase 5) |
| 54 | 74 | tail | zero | free |

## B0 — chromatogram fast-filter summary (128 bytes)

| Offset | Size | Field | Type / sentinel | State |
|---|---|---|---|---|
| 0 | 8 | lowest_mz | f64 | used |
| 8 | 8 | highest_mz | f64 | used |
| 16 | 8 | lowest_wavelength | f64 | used |
| 24 | 8 | highest_wavelength | f64 | used |
| 32 | 8 | lowest_ion_mobility | f64 | used |
| 40 | 8 | highest_ion_mobility | f64 | used |
| 48 | 1 | polarity | u8 | used |
| 49 | 8 | precursor_mz | f64, NaN = unknown | reserved (Phase 5) |
| 57 | 8 | product_mz | f64, NaN = unknown | reserved (Phase 5) |
| 65 | 63 | tail | zero | free |

A0 and B0 are a pure acceleration cache: every used field is derived from the item's
metadata and the same value is stored in C/D under its PSI accession. A0/B0 can be
rebuilt from C/D. There is no per-record checksum, and the header crc does not cover
A0/B0.

## A1 / B1 — array index (16 bytes per item)

| Offset | Size | Field |
|---|---|---|
| 0 | 8 | first array ref index (into A2/B2) |
| 8 | 8 | array ref count |

## A2 / B2 — array refs (32 bytes per array)

| Offset | Size | Field |
|---|---|---|
| 0 | 8 | element offset (inside the block) |
| 8 | 8 | element count |
| 16 | 4 | block id (into the container directory) |
| 20 | 4 | array accession |
| 24 | 1 | dtype (see codes) |
| 25 | 1 | array filter (PackingId, see codes) |
| 26 | 4 | encoded length (0 = fixed-width; >0 = variable-length byte count) |
| 30 | 2 | reserved, zero |

Many refs may point at the same block id and element offset; identical arrays may be
shared. The reader does not care whether arrays are shared.

## Array containers (packed_spectra, packed_chroms)

Layout: `[block][block]…[block][block directory]`. Blocks are grouped by element
stride (1, 2, 4, 8 bytes); a block fills up to the target block size, then seals. An
array larger than the target gets its own dedicated block. Each block is compressed
independently when compression is on.

Block directory entry (32 bytes), one per block:

| Offset | Size | Field |
|---|---|---|
| 0 | 8 | payload offset (relative to container start) |
| 8 | 8 | payload size |
| 16 | 8 | uncompressed length |
| 24 | 4 | crc32 of the stored payload |
| 28 | 4 | reserved, zero |

## C / D — grouped metadata

Layout: `[group payload]…[group payload][group directory]`. Items are split into
fixed groups of `metadata group size` (8192) items. Each group is serialized, then
compressed independently when compression is on. A group's value pools are local: no
pool crosses a group boundary.

Group directory entry (32 bytes), one per group:

| Offset | Size | Field |
|---|---|---|
| 0 | 8 | payload offset (relative to section start) |
| 8 | 8 | payload size |
| 16 | 8 | uncompressed size |
| 24 | 4 | crc32 of the stored payload |
| 28 | 4 | reserved, zero |

Decompressed group body. First a 12-byte header:

| Offset | Size | Field |
|---|---|---|
| 0 | 4 | meta_count (rows in this group) |
| 4 | 4 | numeric_count |
| 8 | 4 | string_count |

Then column arrays in this order (let `n = meta_count`, `g = items in this group`):

| Array | Type | Length |
|---|---|---|
| local index offsets | u32 | g + 1 |
| ids | u32 | n |
| parent ids | u32 | n |
| tag ids | u8 | n |
| ref codes | u8 | n |
| accession numbers | u32 | n |
| unit ref codes | u8 | n |
| unit accession numbers | u32 | n |
| value kinds | u8 | n |
| value indices | u32 | n |
| numeric values | f64 | numeric_count |
| string offsets | u32 | string_count |
| string lengths | u32 | string_count |
| string bytes | u8 | sum of string lengths |

`value kind`: 0 = numeric, 1 = string, 2 = empty. `value index` points into the
numeric or string pool for that row.

## E — global metadata (monolithic)

One compressed section. It does not scale with item count, so it is not grouped. It
starts with a 32-byte counts header, then the same column arrays as a group body
(but for the whole section, with `index offsets` of length `items + 1`).

Counts header (32 bytes): nine u16 values in this order — file descriptions, ref
param groups, samples, instrument configs, software, data processing, acquisition
settings, cvs, runs — then 14 reserved zero bytes.

## Codes

Compression codec (header byte 11): `0` none, `1` zstd.

dtype (A2/B2 byte 24): `1` f64, `2` f32, `3` f16, `4` i16, `5` i32, `6` i64.

Array filter / PackingId (header byte 13, A2/B2 byte 25): `0` raw, `1` byte shuffle,
`2` delta shuffle.

## Integrity and validation

- The file starts with the signature and ends with the trailer `CINOI\0\0\0`.
- Header crc32 (bytes 1008..1012… at 1020) covers bytes 0..1020.
- C, D, E each have a crc32 in the header over their whole section.
- Every block and every group carries its own crc32.
- `total file size` equals the real file length.
- Sections do not overlap, are 8-byte aligned, and sit after the header.

Validation modes:

- Open: cheap directory checks only — directory bounds, each payload ends strictly
  before its directory, and `sum(group uncompressed size)` equals the header total. No
  decompression.
- Random access: decode and checksum only the target group and the target item's
  blocks.
- Full read / strict: decode all groups and assert the full row, numeric, and string
  totals against the header.

## v1 limits

- Header item counts (spectra, chromatograms): u64.
- Metadata-backed reconstruction is bounded by u32: item index and node ids are u32,
  so faithful reconstruction is limited to roughly 250–400 million items.
- Rows, numeric, string counts per section: u32.
- Global counts: u16 (max 65535 each).
- String pool offsets: u32 (max 4 GiB per section).

## Extensibility (the only allowed growth)

The reserved tail of A0 and B0 may gain optional fields. Rules:

- A0 and B0 stay exactly 128 bytes forever.
- Each tail slot, once assigned an offset, keeps that offset, type, sentinel, unit,
  and canonical metadata accession forever, and is never reused for another meaning.
- A slot is used, reserved, or free (zero).
- Readers ignore slots they do not know. Writers zero any tail bytes they do not set.
- The header reserved area stays checked-for-zero and is off-limits. Anything there is
  a real format change.
