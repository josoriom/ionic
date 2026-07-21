# IONIC — overview

IONIC stores one mass-spectrometry run in one binary file: spectra, chromatograms, and their metadata. It is built to open fast and to read very large files without loading them whole.

This page explains the idea. For the exact byte layout, read the spec for the current version: [`v0.md`](v0.md). Every version keeps its own file in `spec/`; this link always points to the latest one.

## One file, found through the header

The file opens with a fixed 1024-byte header. The header holds the global settings and, for every section, the byte offset and length used to find it. A reader trusts the header, not the order of sections on disk. So the parts can be laid out in any order, the file can be written in one pass, and the header is patched in last.

The file starts with the signature `IONIC\0\0\0` and ends with the trailer `\0\0\0CINOI`, and the header stores the total file size — together these catch a truncated or wrong file at once. The header, each metadata section, every data block, and the block directory that locates them all carry a CRC-32, so a reader can check a part before it trusts it.

## The parts

- **Array blocks** — the heavy data. The numbers (m/z, intensity, time) are packed into typed blocks and compressed. Spectra and chromatograms each get their own container, with a directory of blocks at the end. A reader decompresses only the blocks it needs.
- **A0/A1/A2/A3** (spectra) and **B0/B1/B2/B3** (chromatograms) — small fixed-size tables.
  - A0/B0 hold a short summary per item (retention time, base peak, polarity, and a few more). They are not compressed, so a reader can filter items without decoding any data.
  - A1/B1 are the m/z map: the lower and upper m/z of every window, so a reader skips the windows outside the range it wants.
  - A2/B2 point from an item to its list of arrays.
  - A3/B3 describe each array segment: which block holds it, how many elements it has, its number type, and its filter. The chromatogram B-tables mirror the A-tables and their bodies are still being revised.
- **C/D/E** — metadata in columns. Every CV parameter is stored by its accession (for example `MS:1000285`, not the name "total ion current"). C is for spectra, D for chromatograms, E for global metadata such as instrument, software, and samples.

## How a read works

1. **Filter** — read A0/B0 and keep the items you want. Nothing is decoded yet.
2. **Locate** — for a kept item, read A2/A3 to find its arrays and the blocks that hold them, and A1 to skip m/z windows outside the range you want.
3. **Decode** — decompress only those blocks and read the arrays.
4. **Resolve metadata** — read the item's parameters from the columns in C/D. This is the columnar model, explained next.

A spectrum's arrays are cut into ordered **segments by m/z window**. Segments in the same window are routed to shared blocks across spectra, so an extracted-ion read opens only the blocks whose window overlaps the requested m/z range. The **A1** index holds the lower and upper m/z of every window, so a reader decodes only the windows that touch the range and skips the rest. The m/z window read is a convenience wrapper over a generic accession-based window read. Small arrays are left whole. The exact bytes are in [`v0.md`](v0.md).

## Metadata is columnar

Instead of keeping each parameter as its own small object, IONIC lays every field down its own column — one long array per field — and stores the columns side by side. Columns hold one kind of value, so they pack and compress far better than mixed objects, and a reader can load only the columns it needs.

Every CV parameter becomes one **row**. All rows share the same columns: find the rows for an item, read across those columns, and you have everything the item carries. The exact column names, types, and order are in [`v0.md`](v0.md). C holds these columns for spectra, D for chromatograms, E for global metadata.

## A0/B0 are only a cache

Every value in A0/B0 also lives in C/D under its accession, so A0/B0 can be rebuilt from C/D at any time. They exist only to make filtering fast. This is why they carry no CRC and may grow new fields safely — the rules for that growth are in [`v0.md`](v0.md).
