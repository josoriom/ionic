# IONIC — overview

IONIC stores one mass-spectrometry run in one binary file: spectra, chromatograms, and their metadata. It is built to open fast and to read very large files without loading them whole.

This page explains the idea. For the exact byte layout, read the spec for the current version: [`v1.md`](v1.md). Every version keeps its own file in `spec/`; this link always points to the latest one.

## One file, found through the header

The file opens with a fixed 1024-byte header. The header holds the global settings and, for every section, the byte offset and length used to find it. A reader trusts the header, not the order of sections on disk. So the parts can be laid out in any order, the file can be written in one pass, and the header is patched in last.

The file starts with the signature `IONIC\0\0\0` and ends with the trailer `\0\0\0CINOI`, and the header stores the total file size — together these catch a truncated or wrong file at once. The header, each metadata section, every data block, and the block directory that locates them all carry a CRC-32, so a reader can check a part before it trusts it.

## The parts

- **Array blocks** — the heavy data. The numbers (m/z, intensity, time) are packed into typed blocks and compressed. Spectra and chromatograms each get their own container, with a directory of blocks at the end. A reader decompresses only the blocks it needs.
- **A0/A1/A2** (spectra) and **B0/B1/B2** (chromatograms) — small fixed-size tables.
  - A0/B0 hold a short summary per item (retention time, base peak, polarity, and a few more). They are not compressed, so a reader can filter items without decoding any data.
  - A1/B1 point from an item to its list of arrays.
  - A2/B2 describe each array: where it lives, how many elements it has, its number type, and its filter.
  - A3 is optional. When a big spectrum array is cut into segments, A3 holds the low and high bounds of each segment for a searchable axis (e.g., m/z, mobility, time) so a reader can skip the segments outside the range it wants.
  - B3 is the chromatogram twin of A3. It is reserved but not written yet.
- **C/D/E** — metadata in columns. Every CV parameter is stored by its accession (for example `MS:1000285`, not the name "total ion current"). C is for spectra, D for chromatograms, E for global metadata such as instrument, software, and samples.

## How a read works

1. **Filter** — read A0/B0 and keep the items you want. Nothing is decoded yet.
2. **Locate** — for a kept item, read A1/A2 to find its arrays and the blocks that hold them.
3. **Decode** — decompress only those blocks and read the arrays.
4. **Resolve metadata** — read the item's parameters from the columns in C/D. This is the columnar model, explained next.

A large array can be cut into ordered **segments**, each its own block (one A2 ref per segment). A small optional index, **A3**, holds the low and high bounds of every segment for a searchable axis, so a reader can decode only the segments that touch a requested value range and skip the rest. Readers may request a value window by array accession — the common m/z window API is a convenience wrapper over the generic accession-based window read. Small arrays are left whole. The exact bytes are in [`v1.md`](v1.md).

## Metadata is columnar

Instead of keeping each parameter as its own small object, IONIC lays every field down its own column — one long array per field — and stores the columns side by side. Columns hold one kind of value, so they pack and compress far better than mixed objects, and a reader can load only the columns it needs.

Every CV parameter becomes one **row**. All rows share the same columns: find the rows for an item, read across those columns, and you have everything the item carries. The exact column names, types, and order are in [`v1.md`](v1.md). C holds these columns for spectra, D for chromatograms, E for global metadata.

## A0/B0 are only a cache

Every value in A0/B0 also lives in C/D under its accession, so A0/B0 can be rebuilt from C/D at any time. They exist only to make filtering fast. This is why they carry no CRC and may grow new fields safely — the rules for that growth are in [`v1.md`](v1.md).
