pub(super) fn shuffle(input: &[u8], output: &mut [u8], stride: usize) {
    match stride {
        2 => shuffle2(input, output),
        4 => shuffle4(input, output),
        8 => shuffle8(input, output),
        _ => super::scalar::shuffle_any(input, output, stride),
    }
}

pub(super) fn unshuffle(input: &[u8], output: &mut [u8], stride: usize) {
    match stride {
        2 => unshuffle2(input, output),
        4 => unshuffle4(input, output),
        8 => unshuffle8(input, output),
        _ => super::scalar::unshuffle_any(input, output, stride),
    }
}

fn shuffle2(input: &[u8], output: &mut [u8]) {
    let half = input.len() / 2;
    let simd_len = half & !15;
    if simd_len > 0 {
        unsafe { shuffle2_sse2(input, output, half, simd_len) };
    }
    for i in simd_len..half {
        output[i] = input[i * 2];
        output[half + i] = input[i * 2 + 1];
    }
}

#[target_feature(enable = "sse2")]
unsafe fn shuffle2_sse2(input: &[u8], output: &mut [u8], half: usize, simd_len: usize) {
    use std::arch::x86_64::*;
    let load_input =
        |offset: usize| unsafe { _mm_loadu_si128(input.as_ptr().add(offset) as *const __m128i) };
    let mut store_output = |offset: usize, value: __m128i| unsafe {
        _mm_storeu_si128(output.as_mut_ptr().add(offset) as *mut __m128i, value)
    };
    let mask_lo = _mm_set1_epi16(0x00FF_u16 as i16);
    let mut i = 0usize;
    while i < simd_len {
        let a = load_input(i * 2);
        let b = load_input(i * 2 + 16);
        let lo = _mm_packus_epi16(_mm_and_si128(a, mask_lo), _mm_and_si128(b, mask_lo));
        let hi = _mm_packus_epi16(_mm_srli_epi16(a, 8), _mm_srli_epi16(b, 8));
        store_output(i, lo);
        store_output(half + i, hi);
        i += 16;
    }
}

fn unshuffle2(input: &[u8], output: &mut [u8]) {
    let half = input.len() / 2;
    let simd_len = half & !15;
    if simd_len > 0 {
        unsafe { unshuffle2_sse2(input, output, half, simd_len) };
    }
    for i in simd_len..half {
        output[i * 2] = input[i];
        output[i * 2 + 1] = input[half + i];
    }
}

#[target_feature(enable = "sse2")]
unsafe fn unshuffle2_sse2(input: &[u8], output: &mut [u8], half: usize, simd_len: usize) {
    use std::arch::x86_64::*;
    let load_input =
        |offset: usize| unsafe { _mm_loadu_si128(input.as_ptr().add(offset) as *const __m128i) };
    let mut store_output = |offset: usize, value: __m128i| unsafe {
        _mm_storeu_si128(output.as_mut_ptr().add(offset) as *mut __m128i, value)
    };
    let mut i = 0usize;
    while i < simd_len {
        let lo = load_input(i);
        let hi = load_input(half + i);
        let out_a = _mm_unpacklo_epi8(lo, hi);
        let out_b = _mm_unpackhi_epi8(lo, hi);
        store_output(i * 2, out_a);
        store_output(i * 2 + 16, out_b);
        i += 16;
    }
}

fn shuffle4(input: &[u8], output: &mut [u8]) {
    let quarter = input.len() / 4;
    let simd_len = quarter & !15;
    if simd_len > 0 {
        unsafe { shuffle4_sse2(input, output, quarter, simd_len) };
    }
    for i in simd_len..quarter {
        output[i] = input[i * 4];
        output[quarter + i] = input[i * 4 + 1];
        output[2 * quarter + i] = input[i * 4 + 2];
        output[3 * quarter + i] = input[i * 4 + 3];
    }
}

#[target_feature(enable = "sse2")]
unsafe fn shuffle4_sse2(input: &[u8], output: &mut [u8], quarter: usize, simd_len: usize) {
    use std::arch::x86_64::*;
    let load_input =
        |offset: usize| unsafe { _mm_loadu_si128(input.as_ptr().add(offset) as *const __m128i) };
    let mut store_output = |offset: usize, value: __m128i| unsafe {
        _mm_storeu_si128(output.as_mut_ptr().add(offset) as *mut __m128i, value)
    };
    let mask = _mm_set1_epi16(0x00FF_u16 as i16);
    let mut i = 0usize;
    while i < simd_len {
        let a = load_input(i * 4);
        let b = load_input(i * 4 + 16);
        let c = load_input(i * 4 + 32);
        let d = load_input(i * 4 + 48);

        let b0a = _mm_packus_epi16(_mm_and_si128(a, mask), _mm_and_si128(b, mask));
        let b0b = _mm_packus_epi16(_mm_and_si128(c, mask), _mm_and_si128(d, mask));
        let b1a = _mm_packus_epi16(_mm_srli_epi16(a, 8), _mm_srli_epi16(b, 8));
        let b1b = _mm_packus_epi16(_mm_srli_epi16(c, 8), _mm_srli_epi16(d, 8));

        let r0 = _mm_packus_epi16(_mm_and_si128(b0a, mask), _mm_and_si128(b0b, mask));
        let r1 = _mm_packus_epi16(_mm_and_si128(b1a, mask), _mm_and_si128(b1b, mask));
        let r2 = _mm_packus_epi16(_mm_srli_epi16(b0a, 8), _mm_srli_epi16(b0b, 8));
        let r3 = _mm_packus_epi16(_mm_srli_epi16(b1a, 8), _mm_srli_epi16(b1b, 8));

        store_output(i, r0);
        store_output(quarter + i, r1);
        store_output(2 * quarter + i, r2);
        store_output(3 * quarter + i, r3);
        i += 16;
    }
}

fn unshuffle4(input: &[u8], output: &mut [u8]) {
    let quarter = input.len() / 4;
    let simd_len = quarter & !15;
    if simd_len > 0 {
        unsafe { unshuffle4_sse2(input, output, quarter, simd_len) };
    }
    for i in simd_len..quarter {
        output[i * 4] = input[i];
        output[i * 4 + 1] = input[quarter + i];
        output[i * 4 + 2] = input[2 * quarter + i];
        output[i * 4 + 3] = input[3 * quarter + i];
    }
}

#[target_feature(enable = "sse2")]
unsafe fn unshuffle4_sse2(input: &[u8], output: &mut [u8], quarter: usize, simd_len: usize) {
    use std::arch::x86_64::*;
    let load_input =
        |offset: usize| unsafe { _mm_loadu_si128(input.as_ptr().add(offset) as *const __m128i) };
    let mut store_output = |offset: usize, value: __m128i| unsafe {
        _mm_storeu_si128(output.as_mut_ptr().add(offset) as *mut __m128i, value)
    };
    let mut i = 0usize;
    while i < simd_len {
        let b0 = load_input(i);
        let b1 = load_input(quarter + i);
        let b2 = load_input(2 * quarter + i);
        let b3 = load_input(3 * quarter + i);

        let s0lo = _mm_unpacklo_epi8(b0, b1);
        let s0hi = _mm_unpackhi_epi8(b0, b1);
        let s1lo = _mm_unpacklo_epi8(b2, b3);
        let s1hi = _mm_unpackhi_epi8(b2, b3);

        let r0 = _mm_unpacklo_epi16(s0lo, s1lo);
        let r1 = _mm_unpackhi_epi16(s0lo, s1lo);
        let r2 = _mm_unpacklo_epi16(s0hi, s1hi);
        let r3 = _mm_unpackhi_epi16(s0hi, s1hi);

        store_output(i * 4, r0);
        store_output(i * 4 + 16, r1);
        store_output(i * 4 + 32, r2);
        store_output(i * 4 + 48, r3);
        i += 16;
    }
}

fn shuffle8(input: &[u8], output: &mut [u8]) {
    let eighth = input.len() / 8;
    let simd_len = eighth & !15;
    if simd_len > 0 {
        unsafe { shuffle8_sse2(input, output, eighth, simd_len) };
    }
    for i in simd_len..eighth {
        for b in 0..8usize {
            output[b * eighth + i] = input[i * 8 + b];
        }
    }
}

#[target_feature(enable = "sse2")]
unsafe fn shuffle8_sse2(input: &[u8], output: &mut [u8], eighth: usize, simd_len: usize) {
    use std::arch::x86_64::*;
    let load_input =
        |offset: usize| unsafe { _mm_loadu_si128(input.as_ptr().add(offset) as *const __m128i) };
    let mut store_output = |offset: usize, value: __m128i| unsafe {
        _mm_storeu_si128(output.as_mut_ptr().add(offset) as *mut __m128i, value)
    };
    let mask = _mm_set1_epi16(0x00FF_u16 as i16);
    let mut i = 0usize;
    while i < simd_len {
        let c0 = load_input(i * 8);
        let c1 = load_input(i * 8 + 16);
        let c2 = load_input(i * 8 + 32);
        let c3 = load_input(i * 8 + 48);
        let c4 = load_input(i * 8 + 64);
        let c5 = load_input(i * 8 + 80);
        let c6 = load_input(i * 8 + 96);
        let c7 = load_input(i * 8 + 112);

        let s0 = _mm_packus_epi16(_mm_and_si128(c0, mask), _mm_and_si128(c1, mask));
        let s1 = _mm_packus_epi16(_mm_srli_epi16(c0, 8), _mm_srli_epi16(c1, 8));
        let s2 = _mm_packus_epi16(_mm_and_si128(c2, mask), _mm_and_si128(c3, mask));
        let s3 = _mm_packus_epi16(_mm_srli_epi16(c2, 8), _mm_srli_epi16(c3, 8));
        let s4 = _mm_packus_epi16(_mm_and_si128(c4, mask), _mm_and_si128(c5, mask));
        let s5 = _mm_packus_epi16(_mm_srli_epi16(c4, 8), _mm_srli_epi16(c5, 8));
        let s6 = _mm_packus_epi16(_mm_and_si128(c6, mask), _mm_and_si128(c7, mask));
        let s7 = _mm_packus_epi16(_mm_srli_epi16(c6, 8), _mm_srli_epi16(c7, 8));

        let t0 = _mm_packus_epi16(_mm_and_si128(s0, mask), _mm_and_si128(s2, mask));
        let t1 = _mm_packus_epi16(_mm_srli_epi16(s0, 8), _mm_srli_epi16(s2, 8));
        let t2 = _mm_packus_epi16(_mm_and_si128(s1, mask), _mm_and_si128(s3, mask));
        let t3 = _mm_packus_epi16(_mm_srli_epi16(s1, 8), _mm_srli_epi16(s3, 8));
        let t4 = _mm_packus_epi16(_mm_and_si128(s4, mask), _mm_and_si128(s6, mask));
        let t5 = _mm_packus_epi16(_mm_srli_epi16(s4, 8), _mm_srli_epi16(s6, 8));
        let t6 = _mm_packus_epi16(_mm_and_si128(s5, mask), _mm_and_si128(s7, mask));
        let t7 = _mm_packus_epi16(_mm_srli_epi16(s5, 8), _mm_srli_epi16(s7, 8));

        let r0 = _mm_packus_epi16(_mm_and_si128(t0, mask), _mm_and_si128(t4, mask));
        let r4 = _mm_packus_epi16(_mm_srli_epi16(t0, 8), _mm_srli_epi16(t4, 8));
        let r2 = _mm_packus_epi16(_mm_and_si128(t1, mask), _mm_and_si128(t5, mask));
        let r6 = _mm_packus_epi16(_mm_srli_epi16(t1, 8), _mm_srli_epi16(t5, 8));
        let r1 = _mm_packus_epi16(_mm_and_si128(t2, mask), _mm_and_si128(t6, mask));
        let r5 = _mm_packus_epi16(_mm_srli_epi16(t2, 8), _mm_srli_epi16(t6, 8));
        let r3 = _mm_packus_epi16(_mm_and_si128(t3, mask), _mm_and_si128(t7, mask));
        let r7 = _mm_packus_epi16(_mm_srli_epi16(t3, 8), _mm_srli_epi16(t7, 8));

        store_output(i, r0);
        store_output(eighth + i, r1);
        store_output(2 * eighth + i, r2);
        store_output(3 * eighth + i, r3);
        store_output(4 * eighth + i, r4);
        store_output(5 * eighth + i, r5);
        store_output(6 * eighth + i, r6);
        store_output(7 * eighth + i, r7);
        i += 16;
    }
}

fn unshuffle8(input: &[u8], output: &mut [u8]) {
    let eighth = input.len() / 8;
    let simd_len = eighth & !15;
    if simd_len > 0 {
        unsafe { unshuffle8_sse2(input, output, eighth, simd_len) };
    }
    for i in simd_len..eighth {
        for b in 0..8usize {
            output[i * 8 + b] = input[b * eighth + i];
        }
    }
}

#[target_feature(enable = "sse2")]
unsafe fn unshuffle8_sse2(input: &[u8], output: &mut [u8], eighth: usize, simd_len: usize) {
    use std::arch::x86_64::*;
    let load_input =
        |offset: usize| unsafe { _mm_loadu_si128(input.as_ptr().add(offset) as *const __m128i) };
    let mut store_output = |offset: usize, value: __m128i| unsafe {
        _mm_storeu_si128(output.as_mut_ptr().add(offset) as *mut __m128i, value)
    };
    let mut i = 0usize;
    while i < simd_len {
        let cols = [
            load_input(i),
            load_input(eighth + i),
            load_input(2 * eighth + i),
            load_input(3 * eighth + i),
            load_input(4 * eighth + i),
            load_input(5 * eighth + i),
            load_input(6 * eighth + i),
            load_input(7 * eighth + i),
        ];

        let s0 = _mm_unpacklo_epi8(cols[0], cols[1]);
        let s1 = _mm_unpackhi_epi8(cols[0], cols[1]);
        let s2 = _mm_unpacklo_epi8(cols[2], cols[3]);
        let s3 = _mm_unpackhi_epi8(cols[2], cols[3]);
        let s4 = _mm_unpacklo_epi8(cols[4], cols[5]);
        let s5 = _mm_unpackhi_epi8(cols[4], cols[5]);
        let s6 = _mm_unpacklo_epi8(cols[6], cols[7]);
        let s7 = _mm_unpackhi_epi8(cols[6], cols[7]);

        let t0 = _mm_unpacklo_epi16(s0, s2);
        let t1 = _mm_unpackhi_epi16(s0, s2);
        let t2 = _mm_unpacklo_epi16(s1, s3);
        let t3 = _mm_unpackhi_epi16(s1, s3);
        let t4 = _mm_unpacklo_epi16(s4, s6);
        let t5 = _mm_unpackhi_epi16(s4, s6);
        let t6 = _mm_unpacklo_epi16(s5, s7);
        let t7 = _mm_unpackhi_epi16(s5, s7);

        let results = [
            _mm_unpacklo_epi32(t0, t4),
            _mm_unpackhi_epi32(t0, t4),
            _mm_unpacklo_epi32(t1, t5),
            _mm_unpackhi_epi32(t1, t5),
            _mm_unpacklo_epi32(t2, t6),
            _mm_unpackhi_epi32(t2, t6),
            _mm_unpacklo_epi32(t3, t7),
            _mm_unpackhi_epi32(t3, t7),
        ];
        for (r, reg) in results.iter().enumerate() {
            store_output(i * 8 + r * 16, *reg);
        }
        i += 16;
    }
}
