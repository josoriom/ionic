#[inline]
fn load128(input: &[u8], offset: usize) -> std::arch::wasm32::v128 {
    unsafe {
        std::arch::wasm32::v128_load(input.as_ptr().add(offset) as *const std::arch::wasm32::v128)
    }
}

#[inline]
fn store128(output: &mut [u8], offset: usize, value: std::arch::wasm32::v128) {
    unsafe {
        std::arch::wasm32::v128_store(
            output.as_mut_ptr().add(offset) as *mut std::arch::wasm32::v128,
            value,
        )
    }
}

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
    use std::arch::wasm32::*;
    let half = input.len() / 2;
    let simd_len = half & !15;
    let mut i = 0usize;
    while i < simd_len {
        let a = load128(input, i * 2);
        let b = load128(input, i * 2 + 16);
        let lo = u8x16_shuffle::<0, 2, 4, 6, 8, 10, 12, 14, 16, 18, 20, 22, 24, 26, 28, 30>(a, b);
        let hi = u8x16_shuffle::<1, 3, 5, 7, 9, 11, 13, 15, 17, 19, 21, 23, 25, 27, 29, 31>(a, b);
        store128(output, i, lo);
        store128(output, half + i, hi);
        i += 16;
    }
    for i in simd_len..half {
        output[i] = input[i * 2];
        output[half + i] = input[i * 2 + 1];
    }
}

fn unshuffle2(input: &[u8], output: &mut [u8]) {
    use std::arch::wasm32::*;
    let half = input.len() / 2;
    let simd_len = half & !15;
    let mut i = 0usize;
    while i < simd_len {
        let lo = load128(input, i);
        let hi = load128(input, half + i);
        let a = u8x16_shuffle::<0, 16, 1, 17, 2, 18, 3, 19, 4, 20, 5, 21, 6, 22, 7, 23>(lo, hi);
        let b =
            u8x16_shuffle::<8, 24, 9, 25, 10, 26, 11, 27, 12, 28, 13, 29, 14, 30, 15, 31>(lo, hi);
        store128(output, i * 2, a);
        store128(output, i * 2 + 16, b);
        i += 16;
    }
    for i in simd_len..half {
        output[i * 2] = input[i];
        output[i * 2 + 1] = input[half + i];
    }
}

fn shuffle4(input: &[u8], output: &mut [u8]) {
    use std::arch::wasm32::*;
    let quarter = input.len() / 4;
    let simd_len = quarter & !15;
    let mut i = 0usize;
    while i < simd_len {
        let a = load128(input, i * 4);
        let b = load128(input, i * 4 + 16);
        let c = load128(input, i * 4 + 32);
        let d = load128(input, i * 4 + 48);

        let e0 = u8x16_shuffle::<0, 4, 8, 12, 16, 20, 24, 28, 1, 5, 9, 13, 17, 21, 25, 29>(a, b);
        let e1 = u8x16_shuffle::<0, 4, 8, 12, 16, 20, 24, 28, 1, 5, 9, 13, 17, 21, 25, 29>(c, d);
        let r0 = u8x16_shuffle::<0, 1, 2, 3, 4, 5, 6, 7, 16, 17, 18, 19, 20, 21, 22, 23>(e0, e1);
        let r1 =
            u8x16_shuffle::<8, 9, 10, 11, 12, 13, 14, 15, 24, 25, 26, 27, 28, 29, 30, 31>(e0, e1);

        let f0 = u8x16_shuffle::<2, 6, 10, 14, 18, 22, 26, 30, 3, 7, 11, 15, 19, 23, 27, 31>(a, b);
        let f1 = u8x16_shuffle::<2, 6, 10, 14, 18, 22, 26, 30, 3, 7, 11, 15, 19, 23, 27, 31>(c, d);
        let r2 = u8x16_shuffle::<0, 1, 2, 3, 4, 5, 6, 7, 16, 17, 18, 19, 20, 21, 22, 23>(f0, f1);
        let r3 =
            u8x16_shuffle::<8, 9, 10, 11, 12, 13, 14, 15, 24, 25, 26, 27, 28, 29, 30, 31>(f0, f1);

        store128(output, i, r0);
        store128(output, quarter + i, r1);
        store128(output, 2 * quarter + i, r2);
        store128(output, 3 * quarter + i, r3);
        i += 16;
    }
    for i in simd_len..quarter {
        output[i] = input[i * 4];
        output[quarter + i] = input[i * 4 + 1];
        output[2 * quarter + i] = input[i * 4 + 2];
        output[3 * quarter + i] = input[i * 4 + 3];
    }
}

fn unshuffle4(input: &[u8], output: &mut [u8]) {
    use std::arch::wasm32::*;
    let quarter = input.len() / 4;
    let simd_len = quarter & !15;
    let mut i = 0usize;
    while i < simd_len {
        let b0 = load128(input, i);
        let b1 = load128(input, quarter + i);
        let b2 = load128(input, 2 * quarter + i);
        let b3 = load128(input, 3 * quarter + i);

        let s0 = u8x16_shuffle::<0, 16, 1, 17, 2, 18, 3, 19, 4, 20, 5, 21, 6, 22, 7, 23>(b0, b1);
        let s1 =
            u8x16_shuffle::<8, 24, 9, 25, 10, 26, 11, 27, 12, 28, 13, 29, 14, 30, 15, 31>(b0, b1);
        let s2 = u8x16_shuffle::<0, 16, 1, 17, 2, 18, 3, 19, 4, 20, 5, 21, 6, 22, 7, 23>(b2, b3);
        let s3 =
            u8x16_shuffle::<8, 24, 9, 25, 10, 26, 11, 27, 12, 28, 13, 29, 14, 30, 15, 31>(b2, b3);

        let r0 = u8x16_shuffle::<0, 1, 16, 17, 2, 3, 18, 19, 4, 5, 20, 21, 6, 7, 22, 23>(s0, s2);
        let r1 =
            u8x16_shuffle::<8, 9, 24, 25, 10, 11, 26, 27, 12, 13, 28, 29, 14, 15, 30, 31>(s0, s2);
        let r2 = u8x16_shuffle::<0, 1, 16, 17, 2, 3, 18, 19, 4, 5, 20, 21, 6, 7, 22, 23>(s1, s3);
        let r3 =
            u8x16_shuffle::<8, 9, 24, 25, 10, 11, 26, 27, 12, 13, 28, 29, 14, 15, 30, 31>(s1, s3);

        store128(output, i * 4, r0);
        store128(output, i * 4 + 16, r1);
        store128(output, i * 4 + 32, r2);
        store128(output, i * 4 + 48, r3);
        i += 16;
    }
    for i in simd_len..quarter {
        output[i * 4] = input[i];
        output[i * 4 + 1] = input[quarter + i];
        output[i * 4 + 2] = input[2 * quarter + i];
        output[i * 4 + 3] = input[3 * quarter + i];
    }
}

fn shuffle8(input: &[u8], output: &mut [u8]) {
    use std::arch::wasm32::*;
    let eighth = input.len() / 8;
    let simd_len = eighth & !15;
    let mut i = 0usize;
    while i < simd_len {
        let a = load128(input, i * 8);
        let b = load128(input, i * 8 + 16);
        let c = load128(input, i * 8 + 32);
        let d = load128(input, i * 8 + 48);
        let e = load128(input, i * 8 + 64);
        let f = load128(input, i * 8 + 80);
        let g = load128(input, i * 8 + 96);
        let h = load128(input, i * 8 + 112);

        let s0 = u8x16_shuffle::<0, 8, 16, 24, 1, 9, 17, 25, 2, 10, 18, 26, 3, 11, 19, 27>(a, b);
        let s1 = u8x16_shuffle::<4, 12, 20, 28, 5, 13, 21, 29, 6, 14, 22, 30, 7, 15, 23, 31>(a, b);
        let s2 = u8x16_shuffle::<0, 8, 16, 24, 1, 9, 17, 25, 2, 10, 18, 26, 3, 11, 19, 27>(c, d);
        let s3 = u8x16_shuffle::<4, 12, 20, 28, 5, 13, 21, 29, 6, 14, 22, 30, 7, 15, 23, 31>(c, d);
        let s4 = u8x16_shuffle::<0, 8, 16, 24, 1, 9, 17, 25, 2, 10, 18, 26, 3, 11, 19, 27>(e, f);
        let s5 = u8x16_shuffle::<4, 12, 20, 28, 5, 13, 21, 29, 6, 14, 22, 30, 7, 15, 23, 31>(e, f);
        let s6 = u8x16_shuffle::<0, 8, 16, 24, 1, 9, 17, 25, 2, 10, 18, 26, 3, 11, 19, 27>(g, h);
        let s7 = u8x16_shuffle::<4, 12, 20, 28, 5, 13, 21, 29, 6, 14, 22, 30, 7, 15, 23, 31>(g, h);

        let t0 = u8x16_shuffle::<0, 1, 2, 3, 16, 17, 18, 19, 4, 5, 6, 7, 20, 21, 22, 23>(s0, s2);
        let t1 =
            u8x16_shuffle::<8, 9, 10, 11, 24, 25, 26, 27, 12, 13, 14, 15, 28, 29, 30, 31>(s0, s2);
        let t2 = u8x16_shuffle::<0, 1, 2, 3, 16, 17, 18, 19, 4, 5, 6, 7, 20, 21, 22, 23>(s1, s3);
        let t3 =
            u8x16_shuffle::<8, 9, 10, 11, 24, 25, 26, 27, 12, 13, 14, 15, 28, 29, 30, 31>(s1, s3);
        let t4 = u8x16_shuffle::<0, 1, 2, 3, 16, 17, 18, 19, 4, 5, 6, 7, 20, 21, 22, 23>(s4, s6);
        let t5 =
            u8x16_shuffle::<8, 9, 10, 11, 24, 25, 26, 27, 12, 13, 14, 15, 28, 29, 30, 31>(s4, s6);
        let t6 = u8x16_shuffle::<0, 1, 2, 3, 16, 17, 18, 19, 4, 5, 6, 7, 20, 21, 22, 23>(s5, s7);
        let t7 =
            u8x16_shuffle::<8, 9, 10, 11, 24, 25, 26, 27, 12, 13, 14, 15, 28, 29, 30, 31>(s5, s7);

        let r0 = u8x16_shuffle::<0, 1, 2, 3, 4, 5, 6, 7, 16, 17, 18, 19, 20, 21, 22, 23>(t0, t4);
        let r4 = u8x16_shuffle::<8, 9, 10, 11, 12, 13, 14, 15, 24, 25, 26, 27, 28, 29, 30, 31>(t0, t4);
        let r2 = u8x16_shuffle::<0, 1, 2, 3, 4, 5, 6, 7, 16, 17, 18, 19, 20, 21, 22, 23>(t1, t5);
        let r6 = u8x16_shuffle::<8, 9, 10, 11, 12, 13, 14, 15, 24, 25, 26, 27, 28, 29, 30, 31>(t1, t5);
        let r1 = u8x16_shuffle::<0, 1, 2, 3, 4, 5, 6, 7, 16, 17, 18, 19, 20, 21, 22, 23>(t2, t6);
        let r5 = u8x16_shuffle::<8, 9, 10, 11, 12, 13, 14, 15, 24, 25, 26, 27, 28, 29, 30, 31>(t2, t6);
        let r3 = u8x16_shuffle::<0, 1, 2, 3, 4, 5, 6, 7, 16, 17, 18, 19, 20, 21, 22, 23>(t3, t7);
        let r7 = u8x16_shuffle::<8, 9, 10, 11, 12, 13, 14, 15, 24, 25, 26, 27, 28, 29, 30, 31>(t3, t7);

        store128(output, i, r0);
        store128(output, eighth + i, r1);
        store128(output, 2 * eighth + i, r2);
        store128(output, 3 * eighth + i, r3);
        store128(output, 4 * eighth + i, r4);
        store128(output, 5 * eighth + i, r5);
        store128(output, 6 * eighth + i, r6);
        store128(output, 7 * eighth + i, r7);
        i += 16;
    }
    for i in simd_len..eighth {
        for b in 0..8usize {
            output[b * eighth + i] = input[i * 8 + b];
        }
    }
}

fn unshuffle8(input: &[u8], output: &mut [u8]) {
    use std::arch::wasm32::*;
    let eighth = input.len() / 8;
    let simd_len = eighth & !15;
    let mut i = 0usize;
    while i < simd_len {
        let p0 = load128(input, i);
        let p1 = load128(input, eighth + i);
        let p2 = load128(input, 2 * eighth + i);
        let p3 = load128(input, 3 * eighth + i);
        let p4 = load128(input, 4 * eighth + i);
        let p5 = load128(input, 5 * eighth + i);
        let p6 = load128(input, 6 * eighth + i);
        let p7 = load128(input, 7 * eighth + i);

        let s0 = u8x16_shuffle::<0, 16, 1, 17, 2, 18, 3, 19, 4, 20, 5, 21, 6, 22, 7, 23>(p0, p1);
        let s1 =
            u8x16_shuffle::<8, 24, 9, 25, 10, 26, 11, 27, 12, 28, 13, 29, 14, 30, 15, 31>(p0, p1);
        let s2 = u8x16_shuffle::<0, 16, 1, 17, 2, 18, 3, 19, 4, 20, 5, 21, 6, 22, 7, 23>(p2, p3);
        let s3 =
            u8x16_shuffle::<8, 24, 9, 25, 10, 26, 11, 27, 12, 28, 13, 29, 14, 30, 15, 31>(p2, p3);
        let s4 = u8x16_shuffle::<0, 16, 1, 17, 2, 18, 3, 19, 4, 20, 5, 21, 6, 22, 7, 23>(p4, p5);
        let s5 =
            u8x16_shuffle::<8, 24, 9, 25, 10, 26, 11, 27, 12, 28, 13, 29, 14, 30, 15, 31>(p4, p5);
        let s6 = u8x16_shuffle::<0, 16, 1, 17, 2, 18, 3, 19, 4, 20, 5, 21, 6, 22, 7, 23>(p6, p7);
        let s7 =
            u8x16_shuffle::<8, 24, 9, 25, 10, 26, 11, 27, 12, 28, 13, 29, 14, 30, 15, 31>(p6, p7);

        let t0 = u8x16_shuffle::<0, 1, 16, 17, 2, 3, 18, 19, 4, 5, 20, 21, 6, 7, 22, 23>(s0, s2);
        let t1 =
            u8x16_shuffle::<8, 9, 24, 25, 10, 11, 26, 27, 12, 13, 28, 29, 14, 15, 30, 31>(s0, s2);
        let t2 = u8x16_shuffle::<0, 1, 16, 17, 2, 3, 18, 19, 4, 5, 20, 21, 6, 7, 22, 23>(s1, s3);
        let t3 =
            u8x16_shuffle::<8, 9, 24, 25, 10, 11, 26, 27, 12, 13, 28, 29, 14, 15, 30, 31>(s1, s3);
        let t4 = u8x16_shuffle::<0, 1, 16, 17, 2, 3, 18, 19, 4, 5, 20, 21, 6, 7, 22, 23>(s4, s6);
        let t5 =
            u8x16_shuffle::<8, 9, 24, 25, 10, 11, 26, 27, 12, 13, 28, 29, 14, 15, 30, 31>(s4, s6);
        let t6 = u8x16_shuffle::<0, 1, 16, 17, 2, 3, 18, 19, 4, 5, 20, 21, 6, 7, 22, 23>(s5, s7);
        let t7 =
            u8x16_shuffle::<8, 9, 24, 25, 10, 11, 26, 27, 12, 13, 28, 29, 14, 15, 30, 31>(s5, s7);

        store128(
            output,
            i * 8,
            u8x16_shuffle::<0, 1, 2, 3, 16, 17, 18, 19, 4, 5, 6, 7, 20, 21, 22, 23>(t0, t4),
        );
        store128(
            output,
            i * 8 + 16,
            u8x16_shuffle::<8, 9, 10, 11, 24, 25, 26, 27, 12, 13, 14, 15, 28, 29, 30, 31>(t0, t4),
        );
        store128(
            output,
            i * 8 + 32,
            u8x16_shuffle::<0, 1, 2, 3, 16, 17, 18, 19, 4, 5, 6, 7, 20, 21, 22, 23>(t1, t5),
        );
        store128(
            output,
            i * 8 + 48,
            u8x16_shuffle::<8, 9, 10, 11, 24, 25, 26, 27, 12, 13, 14, 15, 28, 29, 30, 31>(t1, t5),
        );
        store128(
            output,
            i * 8 + 64,
            u8x16_shuffle::<0, 1, 2, 3, 16, 17, 18, 19, 4, 5, 6, 7, 20, 21, 22, 23>(t2, t6),
        );
        store128(
            output,
            i * 8 + 80,
            u8x16_shuffle::<8, 9, 10, 11, 24, 25, 26, 27, 12, 13, 14, 15, 28, 29, 30, 31>(t2, t6),
        );
        store128(
            output,
            i * 8 + 96,
            u8x16_shuffle::<0, 1, 2, 3, 16, 17, 18, 19, 4, 5, 6, 7, 20, 21, 22, 23>(t3, t7),
        );
        store128(
            output,
            i * 8 + 112,
            u8x16_shuffle::<8, 9, 10, 11, 24, 25, 26, 27, 12, 13, 14, 15, 28, 29, 30, 31>(t3, t7),
        );
        i += 16;
    }
    for i in simd_len..eighth {
        for b in 0..8usize {
            output[i * 8 + b] = input[b * eighth + i];
        }
    }
}
