# File Header (1024 Bytes)

The Header is the "Root" of the file. It is a fixed-size block that contains global metadata, format flags, and the byte offsets + byte lengths required to navigate to all other sections.

| Offset | Size | Variable Name                     | Type    | Description                                                      |
| :----- | :--- | :-------------------------------- | :------ | :--------------------------------------------------------------- |
| 0      | 8    | `file_signature`                  | u8[8]   | Signature: **"START\0\0\0"**.                                    |
| 8      | 1    | `endianness_flag`                 | u8      | 0 = Little Endian, 1 = Big Endian. Files MUST be little-endian.  |
| 9      | 2    | `format_version`                  | u16     | File format version. 0 = legacy/unversioned, 1 = current format. |
| 11     | 1    | `compression_codec`               | u8      | 0 = none, 1 = zstd.                                              |
| 12     | 1    | `compression_level`               | u8      | Codec level (e.g. 0–21 for zstd).                                |
| 13     | 1    | `array_filter`                    | u8      | 0 = none, 1 = byte shuffle.                                      |
| 14     | 2    | `reserved`                        | u8[2]   | Reserved (0).                                                    |
| 16     | 8    | `target_block_uncompressed_bytes` | u64     | Target uncompressed bytes per block.                             |
| 24     | 8    | `off_spec_entries`                | u64     | Byte offset to Section A (Spectrum Entries).                     |
| 32     | 8    | `len_spec_entries`                | u64     | **On-disk byte length** of Section A.                            |
| 40     | 8    | `off_spec_arrayrefs`              | u64     | Byte offset to Section A1 (Spectrum ArrayRef).                   |
| 48     | 8    | `len_spec_arrayrefs`              | u64     | **On-disk byte length** of Section A1.                           |
| 56     | 8    | `off_chrom_entries`               | u64     | Byte offset to Section B (Chromatogram Entries).                 |
| 64     | 8    | `len_chrom_entries`               | u64     | **On-disk byte length** of Section B.                            |
| 72     | 8    | `off_chrom_arrayrefs`             | u64     | Byte offset to Section B1 (Chromatogram ArrayRef).               |
| 80     | 8    | `len_chrom_arrayrefs`             | u64     | **On-disk byte length** of Section B1.                           |
| 88     | 8    | `off_spec_meta`                   | u64     | Byte offset to Section C (Spectrum Metadata).                    |
| 96     | 8    | `len_spec_meta`                   | u64     | **Compressed (on-disk) byte length** of Section C.               |
| 104    | 8    | `off_chrom_meta`                  | u64     | Byte offset to Section D (Chromatogram Metadata).                |
| 112    | 8    | `len_chrom_meta`                  | u64     | **Compressed (on-disk) byte length** of Section D.               |
| 120    | 8    | `off_global_meta`                 | u64     | Byte offset to Section E (Global Metadata).                      |
| 128    | 8    | `len_global_meta`                 | u64     | **Compressed (on-disk) byte length** of Section E.               |
| 136    | 8    | `off_container_spect`             | u64     | Byte offset to the Spectrum container.                           |
| 144    | 8    | `len_container_spect`             | u64     | **Compressed (on-disk) byte length** of the Spectrum container.  |
| 152    | 8    | `off_container_chrom`             | u64     | Byte offset to the Chrom container.                              |
| 160    | 8    | `len_container_chrom`             | u64     | **Compressed (on-disk) byte length** of the Chrom container.     |
| 168    | 8    | `block_count_spect`               | u64     | Number of BlockDirectory entries in the Spectrum container.      |
| 176    | 8    | `block_count_chrom`               | u64     | Number of BlockDirectory entries in the Chrom container.         |
| 184    | 8    | `spectrum_count`                  | u64     | Total number of spectra stored in the file.                      |
| 192    | 8    | `chrom_count`                     | u64     | Total number of chromatograms stored.                            |
| 200    | 8    | `spec_meta_count`                 | u64     | Total metadata rows for spectra.                                 |
| 208    | 8    | `spec_meta_num_count`             | u64     | Total numeric values in spectrum metadata pool.                  |
| 216    | 8    | `spec_meta_str_count`             | u64     | Total string values in spectrum metadata pool.                   |
| 224    | 8    | `chrom_meta_count`                | u64     | Total metadata rows for chromatograms.                           |
| 232    | 8    | `chrom_meta_num_count`            | u64     | Total numeric values in chromatogram metadata pool.              |
| 240    | 8    | `chrom_meta_str_count`            | u64     | Total string values in chromatogram metadata pool.               |
| 248    | 8    | `global_meta_count`               | u64     | Total metadata rows for global metadata.                         |
| 256    | 8    | `global_meta_num_count`           | u64     | Total numeric values in global metadata pool.                    |
| 264    | 8    | `global_meta_str_count`           | u64     | Total string values in global metadata pool.                     |
| 272    | 8    | `spect_array_count`               | u64     | Number of array types used by Section A1 `array_type`.           |
| 280    | 8    | `chrom_array_count`               | u64     | Number of array types used by Section B1 `array_type`.           |
| 288    | 8    | `spec_meta_uncompressed_bytes`    | u64     | Uncompressed byte size of Section C (Spectrum Metadata).         |
| 296    | 8    | `chrom_meta_uncompressed_bytes`   | u64     | Uncompressed byte size of Section D (Chromatogram Metadata).     |
| 304    | 8    | `global_meta_uncompressed_bytes`  | u64     | Uncompressed byte size of Section E (Global Metadata).           |
| 312    | 8    | `off_filter_index`                | u64     | Byte offset to Section F (Spectrum Filter Index).                |
| 320    | 8    | `len_filter_index`                | u64     | Byte length of Section F (must equal `spectrum_count × 48`).     |
| 328    | 8    | `total_file_size`                 | u64     | Expected total file size in bytes including the trailer.         |
| 336    | 672  | `reserved_ext`                    | u8[672] | Reserved (0).                                                    |
| 1008   | 4    | `spec_meta_crc32`                 | u32     | CRC-32 of compressed Section C.                                  |
| 1012   | 4    | `chrom_meta_crc32`                | u32     | CRC-32 of compressed Section D.                                  |
| 1016   | 4    | `global_meta_crc32`               | u32     | CRC-32 of compressed Section E.                                  |
| 1020   | 4    | `header_crc32`                    | u32     | CRC-32 over header bytes 0–1019.                                 |

# Section A: Spectra (16 Bytes)

This section consists of a contiguous array of **16-byte** entries. It allows the reader to locate the list of arrays that belong to any spectrum.

| Offset | Size | Name            | Type | Description                                           |
| :----- | :--- | :-------------- | :--- | :---------------------------------------------------- |
| 0      | 8    | `arr_ref_start` | u64  | Start index in Section A1 for this spectrum’s arrays. |
| 8      | 8    | `arr_ref_count` | u64  | Number of arrays for this spectrum (N).               |

# Section A1: Spectrum ArrayRef (32 Bytes)

A contiguous array of 32-byte entries. Each entry represents one array belonging to one spectrum.

| Offset | Size | Name          | Type  | Description                                      |
| -----: | ---: | ------------- | ----- | ------------------------------------------------ |
|      0 |    8 | `off_element` | u64   | Starting element index within decompressed block |
|      8 |    8 | `len_element` | u64   | Number of elements in this array                 |
|     16 |    4 | `block_id`    | u32   | Block index within container                     |
|     20 |    4 | `array_type`  | u32   | **MS accession tail** (e.g. `1000515`)           |
|     24 |    1 | `dtype`       | u8    | `1=F64, 2=F32, 3=F16, 4=I16, 5=I32, 6=I64`       |
|     25 |    7 | `reserved`    | u8[7] | Reserved (0)                                     |

# Section B: Chromatograms (16 Bytes)

This section consists of a contiguous array of 16-byte entries. It allows the reader to locate the list of arrays that belong to any chromatogram.

| Offset | Size | Name            | Type | Description                                               |
| :----- | :--- | :-------------- | :--- | :-------------------------------------------------------- |
| 0      | 8    | `arr_ref_start` | u64  | Start index in Section B1 for this chromatogram’s arrays. |
| 8      | 8    | `arr_ref_count` | u64  | Number of arrays for this chromatogram (N).               |

# Section B1: Chromatogram ArrayRef (32 Bytes)

A contiguous array of 32-byte entries. Each entry represents one array belonging to one chromatogram.

| Offset | Size | Name          | Type  | Description                                      |
| -----: | ---: | ------------- | ----- | ------------------------------------------------ |
|      0 |    8 | `off_element` | u64   | Starting element index within decompressed block |
|      8 |    8 | `len_element` | u64   | Number of elements in this array                 |
|     16 |    4 | `block_id`    | u32   | Block index within container                     |
|     20 |    4 | `array_type`  | u32   | **MS accession tail** (e.g. `1000515`)           |
|     24 |    1 | `dtype`       | u8    | `1=F64, 2=F32, 3=F16, 4=I16, 5=I32, 6=I64`       |
|     25 |    7 | `reserved`    | u8[7] | Reserved (0)                                     |

# Section C & D: Metadata (Spectra and Chromatograms)

Sections C and D share an identical structure and logic, using a Columnar Storage model to store property-value pairs (CV Parameters). Section C is dedicated to Spectrum metadata, while Section D is dedicated to Chromatogram metadata.

Instead of storing metadata as objects, the format flattens all properties into parallel arrays. Each individual spectrum or chromatogram points to a specific range of indices within these columns to resolve its unique properties.

### Parallel Column Arrays

| Variable Name | Type  | Description                                                                                                                                                                        |
| :------------ | :---- | :--------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **`CI`**      | u32[] | **Contiguous Index**. A "Range Map" defining boundaries. For any item `i`, its properties are <br> stored in the columns between index `CI[i]` and `CI[i+1]`. Size is `count + 1`. |
| `MTI`         | u8[]  | **Metadata Tag ID**. Metadata Tag ID. The tag/context for metadata row `j` using your `TagId` mapping. `255 = Unknown`. Size = `meta_count`.                                       |
| `MOI`         | u32[] | **Metadata Owner ID**. Tag ID.                                                                                                                                                     |
| `MPI`         | u32[] | **Metadata Parent ID**. Parent ID.                                                                                                                                                 |
| `MRI`         | u8[]  | **Metadata Ref ID**. Identifies the ontology (0=MS, 1=UO, 2=NCIT, 3=PEFF, 4=ATTR, 255=None).                                                                                       |
| `MAN`         | u32[] | **Metadata Accession**. The numeric tail of the CV term (e.g., 1000514 for MS:1000514).                                                                                            |
| `MURI`        | u8[]  | **Metadata Unit Reference**. Reference ID for the property's unit.                                                                                                                 |
| `MUAN`        | u32[] | **Metadata Unit Accession**. Numeric tail of the unit's accession number.                                                                                                          |
| `VK`          | u8[]  | **Value Kind**. Categorizes the value: 0=Numeric (f64), 1=String (UTF-8), 2=None/Empty.                                                                                            |
| `VI`          | u32[] | **Value Index**. Pointer into the value pools (`VN` or `VOFF/VLEN`).                                                                                                               |
| `VN`          | f64[] | **Numeric Pool**. A dense array of all floating-point metadata values.                                                                                                             |
| `VOFF`        | u32[] | **String Offset**. The starting byte position of a string in the `VS` pool.                                                                                                        |
| `VLEN`        | u32[] | **String Length**. The length of the string in bytes. MUST NOT exceed 2³¹ − 1.                                                                                                     |
| `VS`          | u8[]  | **String Pool**. Concatenated UTF-8 byte stream for all string values.                                                                                                             |

**Note:** All accessions in `MAN` and `MUAN` (except `ATTR`) are numeric tails of PSI-MS CV terms (e.g., `MS:1000514` → `1000514`); human-readable names are resolved at runtime via a bundled `cv_table.json`.

Metadata is encoded as tightly-packed columns in the following exact order:

`CI | MOI | MPI | MTI | MRI | MAN | MURI | MUAN | VK | VI | VN | VOFF | VLEN | VS`

### Lengths

For **Section C (Spectrum metadata)**:

- `item_count = spectrum_count`
- `meta_count = spec_meta_count`
- `num_count  = spec_meta_num_count`
- `str_count  = spec_meta_str_count`

For **Section D (Chromatogram metadata)**:

- `item_count = chrom_count`
- `meta_count = chrom_meta_count`
- `num_count  = chrom_meta_num_count`
- `str_count  = chrom_meta_str_count`

The element counts for each column are:

- `CI` has `item_count + 1` elements
- `MRI, MOI, MPI, MTI, MAN, MURI, MUAN, VK, VI` each have `meta_count` elements
- `VN` has `num_count` elements
- `VOFF, VLEN` each have `str_count` elements
- `VS` has `VS_len_bytes` bytes

### Deriving `VS_len_bytes`

If `str_count == 0`, then `VS_len_bytes = 0`. Otherwise:

`VS_len_bytes = max_{j : VK[j] == 1} (VOFF[VI[j]] + VLEN[VI[j]])`.

### Scheme

```text
                                    ONE ITEM
                             _______________________________
                            /                               \
                           CI[i]: [ 100 ]              CI[i] + 1: [ 108 ]
                            |                               |
                            '--------------+----------------'
                                            |
                                (Range of Metadata j)
                                            |
j:  ...     100    101    102    103    104    105    106    107     ...
           ____________________________________________________________________________
VK:        [ 0 ]  [ 1 ]  [ 0 ]  [ 2 ]  [ 0 ]  [ 1 ]  [ 0 ]  [ 1 ]
VI:        [ 0 ]  [ 0 ]  [ 1 ]  [ 0 ]  [ 2 ]  [ 1 ]  [ 3 ]  [ 2 ]
           ____________________________________________________________________________
             |      |      |      |      |      |      |      |
  (j 100, VI 0)     |      |      |      |      |      |      '---(j 107, VI 2)
  (j 101, VI 0)-----'      |      |      |      |      '------------(j 106, VI 3)
  (j 102, VI 1)------------'      |      |      '---------------------(j 105, VI 1)
  (j 103, VI 0)-------------------'      '------------------------------(j 104, VI 2)

POOLS ROUTING:
---------------------------------------------------------------------------------------
IF VK = 0 (Numbers)                    IF VK = 1 (Strings)
VI=0 -> VN[0]: 1.2    (j 100)          VI=0 -> Map[0]: VOFF=0, VLEN=5 ("SPEC1") (j 101)
VI=1 -> VN[1]: 445.1  (j 102)          VI=1 -> Map[1]: VOFF=5, VLEN=3 ("MS1")   (j 105)
VI=2 -> VN[2]: 100.0  (j 104)          VI=2 -> Map[2]: VOFF=8, VLEN=4 ("Scan")  (j 107)
VI=3 -> VN[3]: 2.0    (j 106)          IF VK = 2 (None): Skip Pool (j 103)
---------------------------------------------------------------------------------------

DATA POOLS:
VN (Numbers):  [ 1.2 ]  [ 445.1 ]  [ 100.0 ]  [ 2.0 ]
                 #0        #1         #2        #3

String Map:     (Index 0)         (Index 1)         (Index 2)
VOFF Array:    [    0    ]       [    5    ]       [    8    ]
VLEN Array:    [    5    ]       [    3    ]       [    4    ]
                 |                 |                 |
VS (Bytes): ...[ S ][ P ][ E ][ C ][ 1 ][ M ][ S ][ 1 ][ S ][ c ][ a ][ n ] ...
                \__________0__________/ \______1______/ \________2________/

```

### Data Access Pattern

To resolve metadata for an item at index `i`, the reader performs a **two-step lookup**:

1. **Find the Range**: Query `CI[i]` (start) and `CI[i+1]` (end) to determine the "slice" of metadata rows belonging to that item.
2. **Resolve Values**: Loop through that slice and use the `VK` (Value Kind) to select the correct value pool.

```python
# To retrieve properties for Spectrum i:
start = CI[i]
end = CI[i + 1]

for j in range(start, end):
    # Each 'j' is a property (CVParam)
    tag_id = MTI[j]
    ref_id = MRI[j]
    acc_num = MAN[j]

    if VK[j] == 0:
        # Numeric property
        val = VN[VI[j]]
    elif VK[j] == 1:
        # String property
        val = VS[VOFF[VI[j]] : VOFF[VI[j]] + VLEN[VI[j]]]
```

# Section E: Global Metadata

This section stores high-level, non-sequential information (such as software settings, sample descriptions, and instrument configurations). It consists of two parts: a **General Header** that defines the quantity of global items per category, followed by a **Columnar Storage** block containing the actual properties.

### General Header (32 Bytes)

The first 32 bytes of Section E define the number of logical items per global metadata category. The categories map **1-to-1 to mzML's top-level elements** (as defined in mzML 1.1.0) and are processed sequentially by the Contiguous Index (CI).

| Offset | Variable Name            | Type   | mzML Equivalent Element             | Description                                    |
| :----- | :----------------------- | :----- | :---------------------------------- | :--------------------------------------------- |
| 0      | `n_file_description`     | u16    | **`<fileDescription>`**             | Info on the run and spectra types.             |
| 2      | `n_ref_param_groups`     | u16    | **`<referenceableParamGroupList>`** | Groups of CV terms referenced as a unit.       |
| 4      | `n_samples`              | u16    | **`<sampleList>`**                  | Data about the biological or chemical analyte. |
| 6      | `n_instrument_configs`   | u16    | **`<instrumentConfigurationList>`** | Hardware and operational configurations.       |
| 8      | `n_software`             | u16    | **`<softwareList>`**                | History of software tools utilized.            |
| 10     | `n_data_processing`      | u16    | **`<dataProcessingList>`**          | History of processing after raw acquisition.   |
| 12     | `n_acquisition_settings` | u16    | **`<acquisitionSettingsList>`**     | Special parameters such as inclusion lists.    |
| 14     | `n_cvs`                  | u16    | **`<cvList>`**                      | Referenced controlled vocabulary info.         |
| 16     | `n_run`                  | u16    | **`<run>`**                         | Run.                                           |
| 18     | `reserved`               | u8[14] | —                                   | Reserved (0).                                  |

---

### Columnar Storage

Section E utilizes the same **Parallel Column Array** structure (CI, MRI, MTI, MAN, VK, etc.) defined in Sections C & D.

The total number of items indexed by the `CI` array in this section is the sum of all counts provided in the **General Header**.

# Raw Data containers

Each raw-data region is stored as a **container** made of many **compressed blocks**. The file header fields `off_container_*` / `len_container_*` / `block_count_*` locate the container and its BlockDirectory.

## Container Definitions

| Container (File Region) | Header: Offset Field  | Header: Length Field  | Header: Block Count Field | Decompressed Content                                                                              |
| :---------------------- | :-------------------- | :-------------------- | :------------------------ | :------------------------------------------------------------------------------------------------ |
| **Spectrum container**  | `off_container_spect` | `len_container_spect` | `block_count_spect`       | All decompressed element arrays referenced by Section A1 (m/z, intensity, and any extra arrays).  |
| **Chrom container**     | `off_container_chrom` | `len_container_chrom` | `block_count_chrom`       | All decompressed element arrays referenced by Section B1 (time, intensity, and any extra arrays). |

Each container referenced by `off_container_*` / `len_container_*` is laid out as:

`[CompressedBlockBuffer …] + [BlockDirectory (block_count_* × 32B)]`

Where:

- `compressed_buffer_start = off_container_*`
- `directory_bytes = block_count_* × 32`
- `directory_start = off_container_* + len_container_* - (block_count_* × 32)`

`block_id` in Section A1 / Section B1 is an index into this container’s BlockDirectory (0-based).

## Block Directory Entry (32 Bytes)

A contiguous array of 32-byte entries. Each entry describes one compressed block stored inside the container.

| Range  | Name             | Type  | Description                                                                                |
| :----- | :--------------- | :---- | :----------------------------------------------------------------------------------------- |
| 0..7   | `comp_off`       | u64   | Byte offset of this block’s compressed bytes, relative to `compressed_buffer_start`.       |
| 8..15  | `comp_bytes`     | u64   | **Compressed byte length** of this block (number of bytes to read starting at `comp_off`). |
| 16..23 | `uncomp_bytes`   | u64   | **Uncompressed byte length** produced after decompression (exact output size).             |
| 24..27 | `block_checksum` | u32   | CRC-32 of the compressed payload.                                                          |
| 28..31 | `reserved`       | u8[4] | Reserved (0).                                                                              |

# Section F: Spectrum Filter Index (48 Bytes)

Section F is a flat, uncompressed array of fixed-width 48-byte records — one per spectrum in index order. Enables fast filtering by RT, intensity, MS level, and polarity without decompressing any blocks. All unknown or inapplicable values are encoded as `f64::NaN` (floats) or `0` (integers).

| Offset | Size | Name                | Type   | Description                                             |
| :----- | :--- | :------------------ | :----- | :------------------------------------------------------ |
| 0      | 8    | `rt_seconds`        | f64    | Scan start time in seconds (MS:1000016). NaN = unknown. |
| 8      | 8    | `base_peak_mz`      | f64    | Base peak m/z (MS:1000504). NaN = unknown.              |
| 16     | 8    | `base_peak_int`     | f64    | Base peak intensity (MS:1000505). NaN = unknown.        |
| 24     | 8    | `total_ion_current` | f64    | Total ion current (MS:1000285). NaN = unknown.          |
| 32     | 1    | `ms_level`          | u8     | MS level (MS:1000511). 0 = unknown.                     |
| 33     | 1    | `polarity`          | u8     | 0=unknown, 1=positive, 2=negative.                      |
| 34     | 14   | `reserved`          | u8[14] | Reserved (0).                                           |

- The array contains exactly `spectrum_count` records, in spectrum index order.
- The section is stored **uncompressed**. `len_filter_index` must equal `spectrum_count × 48`.

# File Trailer (8 Bytes)

The last 8 bytes of every valid IONIC file are the ASCII magic trailer:

`"END\0\0\0\0\0"`

Decoders MUST verify all 14 conditions before trusting the parsed data:

1. The first 8 bytes match the file signature `"START\0\0\0"`.
2. The `header_crc32` at offset 1020 matches the CRC-32 of header bytes 0–1019.
3. The final 8 bytes match the trailer signature exactly.
4. The actual file size matches `total_file_size` in the header.
5. `spectrum_count × 16 == len_spec_entries`
6. `chrom_count × 16 == len_chrom_entries`
7. `len_spec_arrayrefs % 32 == 0`
8. `len_chrom_arrayrefs % 32 == 0`
9. `block_count_spect × 32 <= len_container_spect`
10. `block_count_chrom × 32 <= len_container_chrom`
11. `len_filter_index == spectrum_count × 48`
12. Every `offset + length` pair falls within `[0, total_file_size − 8]`
13. No two sections overlap — for every pair of sections, `offset_i + length_i <= offset_j` or `offset_j + length_j <= offset_i`.
14. All section offsets are 8-byte aligned.

The encoder MUST write the trailer as the final operation after all sections and after patching the file header.

# Attributes Accession Registry

The `ATTR` (cv_ref code `4`) encodes format-internal attributes that have no PSI-MS equivalent. All accessions use the format `ATTR:<number>`.

| Range                 | Purpose                    | Examples                                       |
| :-------------------- | :------------------------- | :--------------------------------------------- |
| `9,900,001–9,900,999` | CV-list attributes         | `cvID`, `fullName`, `version`, `URI`           |
| `9,910,001–9,910,019` | String element attributes  | `id`, `name`, `ref`, `nativeID`, `softwareRef` |
| `9,910,100–9,910,199` | Numeric element attributes | `count`, `index`, `order`, `msLevel`           |
| `9,920,001–9,929,999` | Structural markers         | Reserved for group ref markers (V-012)         |
| `9,930,001–9,939,999` | Encoded types              | Reserved for userParam encoding (V-013)        |
| `9,940,000–9,999,999` | Reserved                   | Future allocation                              |
