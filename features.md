# Ionic Format Evolution — Implementation Spec

This document describes how to evolve the Ionic binary format with three layered strategies.
Every change respects three hard constraints:

1. **Public API stability** — symbols re-exported from `crates/parser/src/lib.rs` and
   `crates/parser/src/ion/mod.rs` must not change shape.
2. **Old files keep working** — readers built against format v10+ must still open v9 files.
3. **One feature at a time** — each strategy is independently shippable.

---

## 0. Quick reference — extension points already in the format

| Slot                    | Location                                                                                     | Size  | Currently                                                                                     |
| ----------------------- | -------------------------------------------------------------------------------------------- | ----- | --------------------------------------------------------------------------------------------- |
| Format version          | header bytes 9–10 (u16 LE)                                                                   | 2 B   | `9` — [`parse_header.rs:347`](crates/parser/src/ion/decoder/utilities/parse_header.rs)        |
| Per-file codec ID       | header byte 11                                                                               | 1 B   | `0` = zstd — [`parse_header.rs:348`](crates/parser/src/ion/decoder/utilities/parse_header.rs) |
| Per-array packing ID    | `array_filter` byte in every ArrayRef                                                        | 1 B   | `0`/`1`/`2` — [`decode.rs:97–104`](crates/parser/src/ion/decoder/decode.rs)                   |
| Header reserved bytes   | bytes 352–1007                                                                               | 656 B | All zero                                                                                      |
| ArrayRef reserved tail  | bytes 26–31 of each 32-byte record                                                           | 6 B   | Zero                                                                                          |
| BlockDirEntry padding   | bytes 28–31                                                                                  | 4 B   | Zero                                                                                          |
| `FilterType` enum       | [`container_builder.rs:11–32`](crates/parser/src/ion/encoder/utilities/container_builder.rs) | u8    | `None=0, Shuffle=1, DeltaShuffle=2`                                                           |
| `BlockCompressor` trait | [`container_builder.rs:79–85`](crates/parser/src/ion/encoder/utilities/container_builder.rs) | —     | Single impl: zstd                                                                             |

## 0.1 Public API — must not change

```rust
// crates/parser/src/lib.rs
pub use ion::{ChromatogramSummary, SpectrumSummary, decoder, encoder};
pub use ion::decoder::utilities::spectrum_source::{ScanSource, ScanSummary};
pub use mzml::{BinToMzmlError, bin_to_mzml, parse_indexed_mzml, parse_mzml};

// crates/parser/src/ion/mod.rs
pub use decoder::decode::{Decoder, DecoderConfig, Ion, OwnedIon};
pub use encoder::{encode::WritingMode, encode::encode, utilities::FileEncoderOutput};
```

All three strategies are **internal-only** or **additive**. No FFI break for msutils (R / Python / JS).

---

## S1 — Packing trait + specialized algorithms

### Design goals

This section maps directly to the five programming principles requested:

| Principle                 | How S1 applies it                                                                                                                                                    |
| ------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Single Responsibility** | Each packing algorithm lives in its own file and handles only its own encode/decode logic                                                                            |
| **Dependency Inversion**  | High-level encoder/decoder depend on `&dyn Packing`, never on concrete types like `Alp` or `Chimp`                                                                   |
| **Open/Closed**           | Adding a new algorithm = one new file + one new match arm in `packing_for`. No existing file changes                                                                 |
| **Encapsulation**         | External libraries (`bitpacking`, `stream-vbyte`) are called from exactly one private function inside the file that needs them. Swapping means changing one function |
| **Performance**           | `packing_for` returns `&'static dyn Packing`; decode hot path uses a `match` on `PackingId` for inlining, not a vtable call                                          |

---

### External dependencies — what to use vs what to write

| Algorithm                     | Use or write?          | Reason                                                                                                                                            | Crate                  | Downloads |
| ----------------------------- | ---------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------- | --------- |
| **Stream-VByte (for delta²)** | **Use** `stream-vbyte` | 806K downloads, stable API, no ecosystem tie-in                                                                                                   | `stream-vbyte = "0.4"` | 806K      |
| **Bit-packing (for delta²)**  | **Use** `bitpacking`   | 17M downloads, SIMD-accelerated, used in major DBs                                                                                                | `bitpacking = "0.9"`   | 17M       |
| **ALP**                       | **Write** (~400 LOC)   | Only mature implementation is `vortex-alp`, tied to the Vortex database ecosystem — too heavy. Algorithm is well-specified in the 2024 VLDB paper |
| **Chimp**                     | **Write** (~200 LOC)   | No standalone mature crate exists. Algorithm is simple XOR + leading-zero count                                                                   |

**Wrapper rule**: `bitpacking` and `stream-vbyte` are accessed through **one private function each** inside `delta2_vbyte.rs`. No other file imports them. If you swap either library in the future, you change exactly one function.

---

### What this changes conceptually

Today:

```
m/z array  →  [delta¹ + shuffle + zstd]   (FilterType::DeltaShuffle)
intensity  →  [shuffle + zstd]             (FilterType::Shuffle or None)
RT         →  [shuffle + zstd]             (FilterType::Shuffle or None)
```

After S1:

```
m/z array  →  [delta² + VByte]            (PackingId::DeltaSquaredVByte)
intensity  →  [ALP]                        (PackingId::Alp)
RT         →  [Chimp]                      (PackingId::Chimp)
other      →  [delta¹ + shuffle + zstd]   (PackingId::DeltaShuffle — unchanged)
```

The only on-disk change: the `array_filter` byte in every ArrayRef goes from `{0,1,2}` to `{0,1,2,3,4,5}`. Every other structure — header, blocks, indexes, metadata — is untouched.

---

### New folder structure

```
crates/parser/src/ion/packing/
├── mod.rs            Packing trait + PackingId enum + packing_for() strategy table
├── raw.rs            PackingId::Raw  — copy bytes as-is (~30 LOC)
├── byte_shuffle.rs   PackingId::ByteShuffle — move shuffle logic here (~50 LOC)
├── delta_shuffle.rs  PackingId::DeltaShuffle — delta¹ + shuffle + zstd (~80 LOC)
├── delta2_vbyte.rs   PackingId::DeltaSquaredVByte — delta² + VByte, wraps bitpacking + stream-vbyte (~150 LOC)
├── alp.rs            PackingId::Alp — pure Rust ALP (~400 LOC)
└── chimp.rs          PackingId::Chimp — pure Rust Chimp (~200 LOC)
```

---

### Core types (define these first in `packing/mod.rs`)

```rust
// packing/mod.rs

use crate::ion::error::{IonError, IonResult};

/// Numeric dtype token — mirrors the FILE_DTYPE_* constants but is type-safe.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Dtype {
    F64 = 1,
    F32 = 2,
    F16 = 3,
    I16 = 4,
    I32 = 5,
    I64 = 6,
}

impl Dtype {
    pub(crate) fn from_byte(b: u8) -> IonResult<Self> {
        match b {
            1 => Ok(Self::F64), 2 => Ok(Self::F32), 3 => Ok(Self::F16),
            4 => Ok(Self::I16), 5 => Ok(Self::I32), 6 => Ok(Self::I64),
            _ => Err(IonError::from(format!("unknown dtype byte: {b}"))),
        }
    }
    pub(crate) fn byte_stride(self) -> usize {
        match self { Self::F64 | Self::I64 => 8, Self::F32 | Self::I32 => 4, _ => 2 }
    }
}

/// Identifies the packing algorithm stored in every ArrayRef on disk.
/// Adding a new variant here is the ONLY change needed to introduce a new algorithm.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PackingId {
    Raw              = 0,  // no transform — plain LE bytes
    ByteShuffle      = 1,  // byte-transpose before zstd
    DeltaShuffle     = 2,  // delta¹ + byte-transpose + zstd  (current default)
    // S1 additions — format v10+
    DeltaSquaredVByte = 3, // delta² + Stream-VByte residuals
    Alp              = 4,  // Adaptive Lossless Floating-point
    Chimp            = 5,  // XOR-based timeseries float packing
    // Reserve 6..=15 for future numeric codecs.
    // Reserve 16..=31 for future structural transforms.
}

impl PackingId {
    pub(crate) fn from_byte(b: u8) -> IonResult<Self> {
        match b {
            0 => Ok(Self::Raw),            1 => Ok(Self::ByteShuffle),
            2 => Ok(Self::DeltaShuffle),   3 => Ok(Self::DeltaSquaredVByte),
            4 => Ok(Self::Alp),            5 => Ok(Self::Chimp),
            _ => Err(IonError::UnsupportedPacking(b)),
        }
    }
}

/// Typed input to a packing encoder.
/// Avoids unsafe transmutes by carrying the typed slice directly.
pub(crate) enum PackingInput<'a> {
    F32(&'a [f32]),
    F64(&'a [f64]),
    I16(&'a [i16]),
    I32(&'a [i32]),
    I64(&'a [i64]),
    Bytes(&'a [u8]),   // for packings that operate on raw bytes (Raw, ByteShuffle)
}

/// A lossless packing algorithm.
///
/// # Contract
/// - `decode(encode(x)) == x` bit-for-bit for all valid inputs.
/// - `encode` must NOT clear `out`; the caller manages buffer state.
/// - `decode` writes values in the native dtype byte order (LE) into `out`.
pub(crate) trait Packing: Send + Sync {
    fn id(&self) -> PackingId;

    /// Minimum element count for this packing to be beneficial.
    /// Below this, `packing_for` will return `DeltaShuffle` as a safe fallback.
    fn min_input_len(&self) -> usize { 1 }

    /// Returns true if this packing handles its own compression end-to-end.
    /// When true, the container_builder SKIPS shuffle + zstd for this block.
    /// When false, the output of encode() is still passed through shuffle + zstd.
    fn is_generic(&self) -> bool { false }

    /// Whether this packing supports the given dtype.
    fn supports(&self, dtype: Dtype) -> bool;

    /// Encode typed values into packed bytes.
    fn encode(&self, input: PackingInput<'_>, out: &mut Vec<u8>) -> IonResult<()>;

    /// Decode packed bytes back into LE bytes of the native dtype.
    /// `dtype` must match what was passed to `encode`.
    fn decode(&self, input: &[u8], dtype: Dtype, out: &mut Vec<u8>) -> IonResult<()>;
}
```

---

### Strategy table (`packing/mod.rs` continued)

This is the **only place** that decides which algorithm goes on which column.
Adding a new column type or changing the algorithm for an existing one = change one match arm here.

```rust
use crate::accessions::{MZ_ARRAY, INTENSITY_ARRAY, RT_ARRAY};
use super::{delta_shuffle::DELTA_SHUFFLE, delta2_vbyte::DELTA2_VBYTE,
            alp::ALP, chimp::CHIMP, raw::RAW};

/// Returns the packing strategy for a given (array_type, dtype) pair.
///
/// This is the single decision point for the entire encoder.
/// Decoders use PackingId::from_byte() — they never call this.
pub(crate) fn packing_for(array_type: u32, dtype: Dtype, element_count: usize)
    -> &'static dyn Packing
{
    let candidate: &'static dyn Packing = match (array_type, dtype) {
        (MZ_ARRAY,        Dtype::F64) => &DELTA2_VBYTE,
        (INTENSITY_ARRAY, Dtype::F32) => &ALP,
        (INTENSITY_ARRAY, Dtype::F64) => &ALP,
        (RT_ARRAY,        Dtype::F64) => &CHIMP,
        _                             => &DELTA_SHUFFLE,
    };

    // Fallback: if the candidate requires more elements than we have,
    // use DeltaShuffle (always safe for any input size).
    if element_count < candidate.min_input_len() {
        return &DELTA_SHUFFLE;
    }

    candidate
}

/// Returns the packing for decoding — driven entirely by the on-disk PackingId.
/// The decoder never needs to know array_type.
pub(crate) fn packing_by_id(id: PackingId) -> &'static dyn Packing {
    match id {
        PackingId::Raw               => &RAW,
        PackingId::ByteShuffle       => &super::byte_shuffle::BYTE_SHUFFLE,
        PackingId::DeltaShuffle      => &DELTA_SHUFFLE,
        PackingId::DeltaSquaredVByte => &DELTA2_VBYTE,
        PackingId::Alp               => &ALP,
        PackingId::Chimp             => &CHIMP,
    }
}
```

---

### File-by-file modifications

---

#### 1. NEW — `crates/parser/src/ion/packing/mod.rs`

**What**: The Packing trait, PackingId, PackingInput, Dtype, packing_for, packing_by_id.
**Why**: Central abstraction. Every other change either feeds into this or reads from it.
**How**: Full content given in the "Core types" section above. No external imports except `IonError/IonResult`.

---

#### 2. NEW — `crates/parser/src/ion/packing/raw.rs`

**What**: `Raw` packing — copies bytes as-is. Used when compression is disabled.
**Why**: Makes `packing_by_id` total (covers PackingId 0) without a special-case anywhere.

```rust
use super::{Dtype, Packing, PackingId, PackingInput, IonResult};

pub(crate) static RAW: Raw = Raw;
pub(crate) struct Raw;

impl Packing for Raw {
    fn id(&self) -> PackingId { PackingId::Raw }
    fn is_generic(&self) -> bool { true }
    fn supports(&self, _dtype: Dtype) -> bool { true }

    fn encode(&self, input: PackingInput<'_>, out: &mut Vec<u8>) -> IonResult<()> {
        match input {
            PackingInput::Bytes(b) => out.extend_from_slice(b),
            _ => return Err(IonError::from("Raw packing requires Bytes input")),
        }
        Ok(())
    }

    fn decode(&self, input: &[u8], _dtype: Dtype, out: &mut Vec<u8>) -> IonResult<()> {
        out.extend_from_slice(input);
        Ok(())
    }
}
```

---

#### 3. NEW — `crates/parser/src/ion/packing/byte_shuffle.rs`

**What**: `ByteShuffle` packing — delegates to the existing `byte_transpose` module.
**Why**: Wraps the current `FilterType::Shuffle` logic behind the trait. No algorithm change.
**How**: Call `byte_transpose::shuffle_with_tail` / `unshuffle` directly. This file is ~50 LOC.

```rust
use crate::ion::byte_transpose;
use super::{Dtype, Packing, PackingId, PackingInput, IonResult};

pub(crate) static BYTE_SHUFFLE: ByteShuffle = ByteShuffle;
pub(crate) struct ByteShuffle;

impl Packing for ByteShuffle {
    fn id(&self) -> PackingId { PackingId::ByteShuffle }
    // is_generic() = false: block-level zstd still runs after this transform.
    fn supports(&self, _dtype: Dtype) -> bool { true }

    fn encode(&self, input: PackingInput<'_>, out: &mut Vec<u8>) -> IonResult<()> {
        let bytes = input_as_bytes(&input);
        let stride = input_stride(&input);
        out.resize(bytes.len(), 0);
        byte_transpose::shuffle_with_tail(bytes, out, stride);
        Ok(())
    }

    fn decode(&self, input: &[u8], dtype: Dtype, out: &mut Vec<u8>) -> IonResult<()> {
        let stride = dtype.byte_stride();
        out.resize(input.len(), 0);
        byte_transpose::unshuffle(input, out, stride);
        Ok(())
    }
}
```

---

#### 4. NEW — `crates/parser/src/ion/packing/delta_shuffle.rs`

**What**: `DeltaShuffle` — delta¹ on f64 bits, then byte-shuffle, then zstd via BlockCompressor.
**Why**: This is the current behavior for m/z arrays. Moving it here makes it a first-class Packing
with all the same dispatch semantics as S1 codecs.
**How**: Move the logic from `delta_filter.rs` (encode_f64 / decode_f64) here, and keep the
byte-shuffle step delegating to `byte_transpose`. The block-level zstd is **not** called from
inside this Packing — `is_generic()` returns false, so the container_builder applies it.

```rust
use crate::ion::byte_transpose;
use super::{Dtype, Packing, PackingId, PackingInput, IonResult};

pub(crate) static DELTA_SHUFFLE: DeltaShuffle = DeltaShuffle;
pub(crate) struct DeltaShuffle;

impl Packing for DeltaShuffle {
    fn id(&self) -> PackingId { PackingId::DeltaShuffle }
    // is_generic() = false — shuffle bytes, then zstd runs at block level.
    fn supports(&self, dtype: Dtype) -> bool { matches!(dtype, Dtype::F64) }

    fn encode(&self, input: PackingInput<'_>, out: &mut Vec<u8>) -> IonResult<()> {
        let PackingInput::F64(values) = input else {
            return Err(IonError::from("DeltaShuffle requires F64 input"));
        };
        // Step 1: delta-encode the f64 bit patterns.
        let mut delta_bytes = Vec::with_capacity(values.len() * 8);
        let mut prev: u64 = 0;
        for &v in values {
            let bits = v.to_bits();
            delta_bytes.extend_from_slice(&bits.wrapping_sub(prev).to_le_bytes());
            prev = bits;
        }
        // Step 2: byte-shuffle the delta-encoded bytes.
        out.resize(delta_bytes.len(), 0);
        byte_transpose::shuffle_with_tail(&delta_bytes, out, 8);
        Ok(())
    }

    fn decode(&self, input: &[u8], _dtype: Dtype, out: &mut Vec<u8>) -> IonResult<()> {
        // Step 1: unshuffle (block decompressor already ran zstd before calling us).
        let mut unshuffled = vec![0u8; input.len()];
        byte_transpose::unshuffle(input, &mut unshuffled, 8);
        // Step 2: delta-decode.
        out.reserve(unshuffled.len() / 8 * 8);
        let mut prev: u64 = 0;
        for chunk in unshuffled.chunks_exact(8) {
            prev = prev.wrapping_add(u64::from_le_bytes(chunk.try_into().unwrap()));
            out.extend_from_slice(&prev.to_le_bytes());
        }
        Ok(())
    }
}
```

> **Note**: after this, delete `crates/parser/src/ion/encoder/utilities/delta_filter.rs`.
> Its logic is fully absorbed here.

---

#### 5. NEW — `crates/parser/src/ion/packing/delta2_vbyte.rs`

**What**: Delta-of-delta + Stream-VByte residuals for m/z arrays.
**Why**: Centroided m/z is nearly evenly spaced. After delta² the residuals are near-zero integers
that Stream-VByte + bitpacking can reduce to 1–2 bytes each instead of 8.
**Dependencies**: `stream-vbyte = "0.4"` and `bitpacking = "0.9"` — used in **private functions
only**. The `Packing` impl never references these crates directly.

```rust
use super::{Dtype, Packing, PackingId, PackingInput, IonResult};

pub(crate) static DELTA2_VBYTE: DeltaSquaredVByte = DeltaSquaredVByte;
pub(crate) struct DeltaSquaredVByte;

impl Packing for DeltaSquaredVByte {
    fn id(&self) -> PackingId { PackingId::DeltaSquaredVByte }
    fn is_generic(&self) -> bool { true }  // handles its own compression
    fn min_input_len(&self) -> usize { 3 }    // delta² needs ≥ 3 values
    fn supports(&self, dtype: Dtype) -> bool { matches!(dtype, Dtype::F64) }

    fn encode(&self, input: PackingInput<'_>, out: &mut Vec<u8>) -> IonResult<()> {
        let PackingInput::F64(values) = input else {
            return Err(IonError::from("DeltaSquaredVByte requires F64 input"));
        };
        let residuals = compute_delta2_residuals(values);
        encode_vbyte_residuals(&residuals, out);
        Ok(())
    }

    fn decode(&self, input: &[u8], _dtype: Dtype, out: &mut Vec<u8>) -> IonResult<()> {
        let residuals = decode_vbyte_residuals(input)?;
        let values = reconstruct_from_delta2(&residuals);
        for v in values {
            out.extend_from_slice(&v.to_le_bytes());
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Private functions — the ONLY place bitpacking and stream-vbyte are used.
// To swap either library: change only these functions.
// ---------------------------------------------------------------------------

fn compute_delta2_residuals(values: &[f64]) -> Vec<i64> {
    let bits: Vec<u64> = values.iter().map(|v| v.to_bits()).collect();
    let deltas: Vec<i64> = bits.windows(2).map(|w| w[1].wrapping_sub(w[0]) as i64).collect();
    // Store the first value and first delta as i64 seeds.
    let mut result = Vec::with_capacity(values.len());
    result.push(bits[0] as i64);         // seed: first raw value
    result.push(deltas[0]);              // seed: first delta
    result.extend(deltas.windows(2).map(|w| w[1].wrapping_sub(w[0])));
    result
}

fn reconstruct_from_delta2(residuals: &[i64]) -> Vec<f64> {
    if residuals.len() < 2 { return vec![]; }
    let mut bits = vec![0u64; residuals.len() - 1];
    bits[0] = residuals[0] as u64;
    let mut prev_delta = residuals[1] as u64;
    for (i, &r) in residuals[2..].iter().enumerate() {
        prev_delta = prev_delta.wrapping_add(r as u64);
        bits[i + 1] = bits[i].wrapping_add(prev_delta);
    }
    bits.iter().map(|&b| f64::from_bits(b)).collect()
}

// Wrapper around stream-vbyte (swap here to change library).
fn encode_vbyte_residuals(residuals: &[i64], out: &mut Vec<u8>) {
    use stream_vbyte::{encode::Scalar, Encode};
    // Zigzag-encode i64 → u64 so small negative residuals stay small.
    let zigzag: Vec<u32> = residuals.iter()
        .map(|&v| ((v << 1) ^ (v >> 63)) as u32)
        .collect();
    let max_bytes = stream_vbyte::max_compressed_len(zigzag.len());
    let old_len = out.len();
    out.resize(old_len + 4 + max_bytes, 0);
    // Write count prefix.
    let count = zigzag.len() as u32;
    out[old_len..old_len + 4].copy_from_slice(&count.to_le_bytes());
    let written = Scalar::encode(&zigzag, &mut out[old_len + 4..]);
    out.truncate(old_len + 4 + written);
}

// Wrapper around stream-vbyte (swap here to change library).
fn decode_vbyte_residuals(input: &[u8]) -> IonResult<Vec<i64>> {
    use stream_vbyte::{decode::Scalar, Decode};
    if input.len() < 4 {
        return Err(IonError::from("DeltaSquaredVByte: truncated input"));
    }
    let count = u32::from_le_bytes(input[..4].try_into().unwrap()) as usize;
    let mut zigzag = vec![0u32; count];
    Scalar::decode(&input[4..], count, &mut zigzag);
    // Zigzag-decode u32 → i64.
    Ok(zigzag.iter().map(|&z| ((z >> 1) as i64) ^ -((z & 1) as i64)).collect())
}
```

---

#### 6. NEW — `crates/parser/src/ion/packing/alp.rs`

**What**: Pure-Rust ALP (Adaptive Lossless Floating-point) for intensity arrays.
**Why**: ALP exploits that scientific floats in the same spectrum share the same exponent.
It stores the exponent once, then encodes only the mantissa deltas.
No external dep: ~400 LOC of bit manipulation.

The ALP algorithm sketch (full implementation goes here):

1. **Sample** the first 64 values to find the dominant exponent pattern.
2. **Encode**: for each f64, extract `(sign, exponent, mantissa)`. Store exponent column + mantissa column separately as LE bytes. Exception-encode values whose exponent differs.
3. **Decode**: reassemble sign + exponent + mantissa per value.

The encode/decode functions are private. Only the `Packing` impl is public (within the crate).

```rust
use super::{Dtype, Packing, PackingId, PackingInput, IonResult};

pub(crate) static ALP: Alp = Alp;
pub(crate) struct Alp;

impl Packing for Alp {
    fn id(&self) -> PackingId { PackingId::Alp }
    fn is_generic(&self) -> bool { true }
    fn min_input_len(&self) -> usize { 64 }
    fn supports(&self, dtype: Dtype) -> bool { matches!(dtype, Dtype::F32 | Dtype::F64) }

    fn encode(&self, input: PackingInput<'_>, out: &mut Vec<u8>) -> IonResult<()> {
        match input {
            PackingInput::F64(values) => alp_encode_f64(values, out),
            PackingInput::F32(values) => alp_encode_f32(values, out),
            _ => Err(IonError::from("ALP requires F32 or F64 input")),
        }
    }

    fn decode(&self, input: &[u8], dtype: Dtype, out: &mut Vec<u8>) -> IonResult<()> {
        match dtype {
            Dtype::F64 => alp_decode_f64(input, out),
            Dtype::F32 => alp_decode_f32(input, out),
            _ => Err(IonError::from("ALP: unsupported dtype on decode")),
        }
    }
}

// ---------------------------------------------------------------------------
// Private implementation — no external libraries.
// Reference: "ALP: Adaptive Lossless Floating-Point Compression" (VLDB 2024).
// ---------------------------------------------------------------------------

fn alp_encode_f64(values: &[f64], out: &mut Vec<u8>) -> IonResult<()> {
    // TODO: implement per the paper
    // Steps: sample → find exponent mode → split stream → encode exceptions
    todo!()
}
fn alp_decode_f64(input: &[u8], out: &mut Vec<u8>) -> IonResult<()> {
    todo!()
}
fn alp_encode_f32(values: &[f32], out: &mut Vec<u8>) -> IonResult<()> { todo!() }
fn alp_decode_f32(input: &[u8], out: &mut Vec<u8>) -> IonResult<()> { todo!() }
```

---

#### 7. NEW — `crates/parser/src/ion/packing/chimp.rs`

**What**: Pure-Rust Chimp (XOR-based) for retention time arrays.
**Why**: RT values change slowly. XOR of adjacent f64 values has many leading zeros → packs tightly.
No external dep: ~200 LOC.

```rust
use super::{Dtype, Packing, PackingId, PackingInput, IonResult};

pub(crate) static CHIMP: Chimp = Chimp;
pub(crate) struct Chimp;

impl Packing for Chimp {
    fn id(&self) -> PackingId { PackingId::Chimp }
    fn is_generic(&self) -> bool { true }
    fn supports(&self, dtype: Dtype) -> bool { matches!(dtype, Dtype::F64) }

    fn encode(&self, input: PackingInput<'_>, out: &mut Vec<u8>) -> IonResult<()> {
        let PackingInput::F64(values) = input else {
            return Err(IonError::from("Chimp requires F64 input"));
        };
        chimp_encode(values, out)
    }

    fn decode(&self, input: &[u8], _dtype: Dtype, out: &mut Vec<u8>) -> IonResult<()> {
        chimp_decode(input, out)
    }
}

// ---------------------------------------------------------------------------
// Private implementation — no external libraries.
// Reference: "Chimp: Efficient Lossless Floating Point Compression for
//             Time Series Databases" (VLDB 2022).
// ---------------------------------------------------------------------------

fn chimp_encode(values: &[f64], out: &mut Vec<u8>) -> IonResult<()> {
    // XOR each value with the previous, then pack leading-zero count + significant bits.
    todo!()
}
fn chimp_decode(input: &[u8], out: &mut Vec<u8>) -> IonResult<()> {
    todo!()
}
```

---

#### 8. MODIFY — `crates/parser/src/ion/mod.rs`

**Where**: after the last `pub(crate) mod` line (currently `pub(crate) mod byte_transpose;`).
**Why**: register the new module so the compiler sees it.
**How**: add one line:

```rust
pub(crate) mod packing;  // add this
```

---

#### 9. MODIFY — `crates/parser/Cargo.toml`

**Where**: `[dependencies]` section.
**Why**: add the two trustworthy external deps, feature-gated so a lean build can opt out.
**How**:

```toml
[features]
default = []
# Enable S1 standalone packings (ALP, Chimp, DeltaSquaredVByte).
# Adds ~100 KB to the binary. Disable for minimal/embedded builds.
s1-packings = ["dep:stream-vbyte", "dep:bitpacking"]

[dependencies]
# Existing deps unchanged ...
stream-vbyte = { version = "0.4", optional = true }
bitpacking   = { version = "0.9", optional = true }
```

Gate the import in `delta2_vbyte.rs`:

```rust
#[cfg(feature = "s1-packings")]
use stream_vbyte::{encode::Scalar as VByteEnc, Encode};
```

If `s1-packings` is disabled, `packing_for` must not return `&DELTA2_VBYTE`. Add a compile-time guard:

```rust
pub(crate) fn packing_for(array_type: u32, dtype: Dtype, element_count: usize)
    -> &'static dyn Packing
{
    #[cfg(feature = "s1-packings")]
    {
        let candidate = match_s1_candidate(array_type, dtype);
        if element_count >= candidate.min_input_len() {
            return candidate;
        }
    }
    // Fallback path — always available.
    &DELTA_SHUFFLE
}
```

---

#### 10. MODIFY — `crates/parser/src/ion/encoder/utilities/container_builder.rs`

**Why**: `FilterType` is replaced by `PackingId`; `make_block` must respect `is_generic()`.

**Step 1 — Replace `FilterType` import with `PackingId`** (lines 11–32):

Delete the `FilterType` enum entirely. Import `PackingId` from the packing module:

```rust
// remove: pub(crate) enum FilterType { ... }
// add:
use crate::ion::packing::{PackingId, Packing};
```

**Step 2 — Update `make_block` signature** (line 419):

```rust
// before:
fn make_block(filter_type: FilterType, block: PendingBlock, ...) -> IonResult<ReadyBlock>

// after:
fn make_block(packing: &dyn Packing, block: PendingBlock, ...) -> IonResult<ReadyBlock>
```

**Step 3 — Update `make_block` body** (lines 428–455):

The current `make_block` always shuffles + compresses. With `is_generic()` packings, skip both:

```rust
CompressionMode::Compressed(compressor) => {
    if packing.is_generic() {
        // Packing already handled full compression. Store bytes as-is.
        ReadyBlock { ..., bytes: block.data }
    } else {
        // Legacy path: shuffle bytes then zstd. Unchanged behavior.
        let shuffled = ...;
        compressor.compress(&shuffled, &mut bytes)?;
        ReadyBlock { ..., bytes }
    }
}
```

**Step 4 — Update `encoder/utilities/mod.rs`** (line 9):

```rust
// remove:
pub(crate) mod delta_filter;

// remove from re-exports:
pub(crate) use container_builder::{..., FilterType};

// add:
pub(crate) use crate::ion::packing::{PackingId, packing_for};
```

---

#### 11. MODIFY — `crates/parser/src/ion/encoder/encode.rs`

**Where**: lines 738–771 (`fill_container` inner loop, dtype + filter resolution).
**Why**: replace the hardcoded `use_delta` branch with a call to `packing_for`.

**Before** (lines 741–756):

```rust
let use_delta = acc == MZ_ARRAY
    && dtype == FILE_DTYPE_F64
    && matches!(data, ArrayData::F64(_))
    && config.compression_is_enabled();
let array_filter = if use_delta { FilterType::DeltaShuffle as u8 } else { 0u8 };
// ...
if use_delta {
    delta_filter::encode_f64(slice, buf);
} else {
    write_array_data(buf, data, dtype);
}
```

**After**:

```rust
use crate::ion::packing::{packing_for, Dtype, PackingInput};

let packing_dtype = Dtype::from_byte(dtype)?;
let packing = if config.compression_is_enabled() {
    packing_for(acc, packing_dtype, data.element_count())
} else {
    &crate::ion::packing::raw::RAW as &dyn Packing
};
let array_filter = packing.id() as u8;

let (block_id, elem_offset) = container.add_item_to_box(
    data.element_count() * elem_bytes,
    elem_bytes,
    packing.is_generic(),   // NEW parameter: skip block-level zstd?
    |buf| {
        let input = typed_input_from_array_data(data, packing_dtype);
        packing.encode(input, buf)
    },
)?;
```

Add the helper:

```rust
fn typed_input_from_array_data<'a>(data: &'a ArrayData, dtype: Dtype) -> PackingInput<'a> {
    match (data, dtype) {
        (ArrayData::F64(s), Dtype::F64) => PackingInput::F64(s),
        (ArrayData::F32(s), Dtype::F32) => PackingInput::F32(s),
        _ => PackingInput::Bytes(data.as_raw_bytes()),
    }
}
```

---

#### 12. MODIFY — `crates/parser/src/ion/decoder/decode.rs`

**Where**: `decode_into` function (lines 1280–1331) and `raw_to_binary_data` (lines 1479–1511).
**Why**: replace the manual `if array_filter == FilterType::DeltaShuffle as u8` branches with
a packing dispatch.

**Before** (lines 1283–1291):

```rust
FILE_DTYPE_F64 => {
    buf.reserve(raw.len() / 8);
    if array_filter == FilterType::DeltaShuffle as u8 {
        delta_filter::decode_f64(raw, buf);
    } else {
        buf.extend(raw.chunks_exact(8).map(...));
    }
}
```

**After**:

```rust
fn decode_into(buf: &mut Vec<f64>, raw: &[u8], dtype: u8, array_filter: u8) {
    use crate::ion::packing::{packing_by_id, PackingId, Dtype};

    let packing_id = PackingId::from_byte(array_filter)
        .unwrap_or(PackingId::Raw);
    let packing = packing_by_id(packing_id);
    let dtype_typed = Dtype::from_byte(dtype).unwrap_or(Dtype::F64);

    // Packing decodes into raw LE bytes of the native dtype.
    let mut raw_typed: Vec<u8> = Vec::with_capacity(raw.len());
    if let Err(e) = packing.decode(raw, dtype_typed, &mut raw_typed) {
        // Log error; leave buf empty — caller handles missing data.
        eprintln!("packing decode error: {e}");
        return;
    }

    // Widen native dtype bytes to f64 (existing behavior, unchanged).
    widen_to_f64(buf, &raw_typed, dtype);
}

fn widen_to_f64(buf: &mut Vec<f64>, raw: &[u8], dtype: u8) {
    // The existing match dtype { FILE_DTYPE_F64 => ..., FILE_DTYPE_F32 => ..., ... }
    // moves here verbatim — no logic change.
}
```

Apply the same pattern to `raw_to_binary_data` (lines 1479–1511).

---

#### 13. MODIFY — `crates/parser/src/ion/decoder/utilities/parse_header.rs`

**Where**: version check (around line 347).
**Why**: gate S1 files behind format version 10. Old readers reject new files with a clear error.

```rust
const FORMAT_VERSION_V9:  u16 = 9;   // current
const FORMAT_VERSION_V10: u16 = 10;  // S1 packings

let version = u16::from_le_bytes(header[HEADER_FORMAT_VERSION..HEADER_FORMAT_VERSION+2]
    .try_into().unwrap());

match version {
    FORMAT_VERSION_V9 | FORMAT_VERSION_V10 => { /* ok */ }
    v => return Err(IonError::UnsupportedFormatVersion(v)),
}
```

Encoder writes `FORMAT_VERSION_V10` only when at least one array used a PackingId >= 3.
Otherwise it stays on V9 — files that don't trigger new packings remain readable by old decoders.

---

#### 14. DELETE — `crates/parser/src/ion/encoder/utilities/delta_filter.rs`

**Why**: logic fully absorbed into `packing/delta_shuffle.rs`.
**How**: move the tests from `delta_filter.rs` into `packing/delta_shuffle.rs` before deleting.

---

### Error variants to add in `crates/parser/src/ion/error.rs`

```rust
IonError::UnsupportedPacking(u8)        // unknown packing byte in ArrayRef
IonError::UnsupportedFormatVersion(u16) // file was written by a newer Ionic
```

These replace the current `format!("unknown filter type byte: {unknown}")` string error.

---

### Format version impact summary

| Encoder output    | Format version | Readable by                   |
| ----------------- | -------------- | ----------------------------- |
| No S1 codecs used | V9 (unchanged) | All existing readers          |
| Any S1 codec used | V10            | Readers built with S1 support |

---

### Tradeoffs

| Pro                                                                            | Con                                                                                                                   |
| ------------------------------------------------------------------------------ | --------------------------------------------------------------------------------------------------------------------- |
| Open/Closed: new algorithm = one new file, zero changes to existing files      | ALP and Chimp must be implemented from scratch (~600 LOC)                                                             |
| External deps isolated to private functions — swap in one place                | `bitpacking` + `stream-vbyte` add ~100 KB to the binary                                                               |
| `is_generic()` cleanly separates transform-only from full-compression packings | `is_generic()` is a naming smell — consider `CompressionSemantic::Transform` vs `::Standalone` if more variants arise |
| Encoder can stay on V9 if no S1 codecs trigger                                 | ALP `min_input_len = 64` means short spectra still use DeltaShuffle                                                   |
| Feature flag keeps lean builds opt-out                                         | Two compilation paths to test                                                                                         |

---

### Phased rollout — each phase is one PR

The reading order (top of S1) is for understanding the design in one sitting.
The **implementation order** below is different: it breaks S1 into four independently
shippable PRs. Each phase compiles, passes the full test suite, and can be reverted
without affecting the others. Do not start phase N+1 until phase N is merged.

| Phase  | Scope                                                                                                                                                                                                      | Format version after merge | Risk                                                        | Files touched                       |
| ------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------- | ----------------------------------------------------------- | ----------------------------------- |
| **P1** | Pure refactor: introduce `Packing` trait, move `FilterType::{None,Shuffle,DeltaShuffle}` into `packing/{raw,byte_shuffle,delta_shuffle}.rs`. Delete `delta_filter.rs`. No new algorithm. No external deps. | V9 (unchanged)             | Low — pure code move                                        | Files 1–4, 8, 10–12, 14             |
| **P2** | Add `DeltaSquaredVByte` behind `s1-packings` feature. Wire format version bump: writer emits V10 only when this codec runs.                                                                                | V9 default, V10 when used  | Medium — first external deps (`stream-vbyte`, `bitpacking`) | File 5, Cargo.toml, parse_header.rs |
| **P3** | Add `Chimp` (pure Rust, ~200 LOC). No new external deps.                                                                                                                                                   | V9 default, V10 when used  | Medium — new algorithm, no deps                             | File 7 only                         |
| **P4** | Add `Alp` (pure Rust, ~400 LOC). No new external deps.                                                                                                                                                     | V9 default, V10 when used  | High — most complex algorithm                               | File 6 only                         |

**Gate per phase**: each phase must pass before the next starts:

- **P1 gate**: a committed V9 fixture file opens after the refactor with **byte-identical** output (decode → re-encode → compare). Benchmarks show ≤2% regression vs main.
- **P2 gate**: round-trip property test passes 10K random inputs. Benchmark on a real centroided m/z array shows ≥15% size reduction vs DeltaShuffle. If not, revert P2 and stop — the assumption was wrong.
- **P3 gate**: same property test + ≥10% size reduction on a real RT array vs DeltaShuffle.
- **P4 gate**: same property test + ≥10% size reduction on a real intensity array vs DeltaShuffle.

**Rollback policy**: any phase that fails its gate is reverted, not patched. P1 is the
only phase whose revert is non-trivial (it touches dispatch sites). P2–P4 are pure
additions and revert by removing one file + one match arm.

---

### Verification plan

1. **Property tests** (`packing/mod.rs` test module): for every `Packing` impl, generate 10K random inputs and assert `decode(encode(x)) == x` bit-for-bit.
2. **Backward-compat fixture test**: commit a tiny V9 `.ion` file to `crates/parser/tests/fixtures/`. A test opens it after the S1 refactor and asserts byte-identical output.
3. **Version rejection test**: write a file with format version 10, attempt to open it with a V9-only decoder, assert `IonError::UnsupportedFormatVersion(10)`.
4. **Benchmark** (`benches/packing_bench.rs`): criterion comparison of `DeltaShuffle` vs `DeltaSquaredVByte` on a 10K-point centroided m/z array. Gate the S1 migration on a measured improvement, not a projected one.
5. **Lean build test**: `cargo build --no-default-features` must compile with zero errors.

---

## S2 — Tiered block sizes + zstd dictionaries + per-column metadata

### S2a — Adaptive block sizes

**File**: [`crates/parser/src/ion/encoder/encode.rs:35`](crates/parser/src/ion/encoder/encode.rs:35)

Replace:

```rust
pub const TARGET_BLOCK_UNCOMPRESSED_BYTES: usize = 32 * 1024 * 1024;
```

With a function keyed on `(array_type, packing_id)`:

```rust
pub(crate) fn target_block_bytes(array_type: u32, packing_id: PackingId) -> usize {
    if packing_id.is_generic() {
        // Standalone packings compress themselves — don't over-size blocks.
        return 1 * 1024 * 1024;
    }
    match array_type {
        MZ_ARRAY        => 4  * 1024 * 1024,
        INTENSITY_ARRAY => 1  * 1024 * 1024,
        RT_ARRAY        => 256 * 1024,
        _               => 4  * 1024 * 1024,
    }
}
```

No format version bump — block sizes are an encoder choice; the decoder reads actual sizes from `BlockDirEntry` (already does today).

### S2b — Zstd dictionaries

**Format version**: bump to V11.

**New header section** (carve from reserved bytes 352–1007):

```rust
const HEADER_OFF_CODEC_DICTS: usize = 352;  // u64 file offset
const HEADER_LEN_CODEC_DICTS: usize = 360;  // u64 byte length
const HEADER_CRC_CODEC_DICTS: usize = 368;  // u32 crc32
```

**Dictionary blob format** (self-describing, versioned):

```
bytes 0-3:   magic = b"DICT"
bytes 4-5:   u16 LE version (start at 1)
bytes 6-7:   u16 LE entry_count
bytes 8+:    [entry_count * 32] entries:
             u32 array_type, u8 dtype, u8 packing_id, u8[2] reserved,
             u64 dict_offset, u64 dict_len, u32 dict_crc32, u32 reserved
then:        concatenated dictionary bytes
```

**New file**: `crates/parser/src/ion/encoder/utilities/dictionary.rs`

- `DictionaryBuilder::observe(array_type, dtype, block_bytes)` — collect samples
- `DictionaryBuilder::train() -> Vec<TrainedDict>` — calls `zstd::dict::from_samples`
- Encoder opt-in via `EncoderConfig::train_dictionaries(true)` (single boolean)

**Decoder** (`decode.rs`, `OwnedIon::open*` path):

```rust
let dicts = if header.format_version() >= 11 && header.dict_section_len() > 0 {
    load_zstd_dicts(&backing_bytes[header.dict_offset()..][..header.dict_len()])
} else {
    HashMap::new()
};
```

### S2c — Per-column metadata compression

**File**: [`crates/parser/src/ion/encoder/utilities/meta_collector.rs:139-141`](crates/parser/src/ion/encoder/utilities/meta_collector.rs:139)

Replace the single `compress_bytes_if_enabled(raw_s, level)` call with per-column compression.
Each of the 13 columns in `PackedMeta` gets its own zstd stream.

A self-describing column header precedes each stream:

```
bytes 0-1:   u16 column_id  (enum: IndexOffsets=0, Ids=1, ..., StringBytes=13)
bytes 2-5:   u32 uncompressed_size
bytes 6-9:   u32 compressed_size
bytes 10-13: u32 crc32
then:        compressed column bytes
```

The decoder can skip any column it doesn't need and decompress lazily via `OnceCell`.

**Format version**: bump to V11 (same as S2b — ship together).

---

## S3 — Arrow IPC read adapter

**What**: A feature-gated `Ion::as_arrow_stream()` that converts the Ionic read path into
an Apache Arrow `RecordBatchReader`. The file format does not change.

**Why**: DuckDB, Polars, and pandas can stream Ionic files without a custom library.
Composes with S1 + S2 — Arrow batches benefit from tighter packing automatically.

**Feature flag**:

```toml
[features]
arrow = ["dep:arrow", "dep:arrow-array", "dep:arrow-ipc"]
```

**New file**: `crates/parser/src/ion/decoder/arrow_view.rs`

- `IonArrowReader<'a>` — implements `Iterator<Item = RecordBatch>`
- Schema: `SpectrumSummary` fields + `mz: List<Float64>` + `intensity: List<Float32>`
- Zero-copy: allocate Arrow `MutableBuffer` 64-byte aligned, write decode output directly into it

**Public re-export** (additive only):

```rust
// crates/parser/src/lib.rs  — additive, behind feature flag
#[cfg(feature = "arrow")]
pub use ion::decoder::arrow_view::{IonArrowReader, IonArrowSchema};
```

**Format version impact**: none — read-only adapter.

**Tradeoffs**:

- Pro: DuckDB / Polars / pandas read Ionic files natively
- Pro: zero format change
- Con: Arrow crate adds ~3 MB to compile time
- Con: useful only if downstream is Arrow-native; pure-Rust consumers gain nothing

---

## Rollout order

| Phase | What                                                                              | Format version | Effort | Risk      |
| ----- | --------------------------------------------------------------------------------- | -------------- | ------ | --------- |
| 1     | S2a — adaptive block sizes                                                        | V9 (no bump)   | 1 wk   | 🟢 Low    |
| 2     | S1 — Packing trait + DeltaShuffle migration (rename + restructure, no new codecs) | V9             | 2 wk   | 🟢 Low    |
| 3     | S1 — DeltaSquaredVByte (m/z)                                                      | V10            | 2 wk   | 🟡 Medium |
| 4     | S1 — ALP (intensity)                                                              | V10            | 3 wk   | 🟡 Medium |
| 5     | S1 — Chimp (RT)                                                                   | V10            | 1 wk   | 🟡 Medium |
| 6     | S2b + S2c — dictionaries + per-column metadata                                    | V11            | 3 wk   | 🟡 Medium |
| 7     | S3 — Arrow adapter                                                                | V11 (no bump)  | 2 wk   | 🟢 Low    |

Phase 2 is pure refactoring — same behavior, new structure. Do it before implementing any new codec.
Phases 3–5 each require the Phase 2 structure to be in place first.
