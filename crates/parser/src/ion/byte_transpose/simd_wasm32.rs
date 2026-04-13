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
        let a = v128_load(input.as_ptr().add(i * 2) as *const v128);
        let b = v128_load(input.as_ptr().add(i * 2 + 16) as *const v128);
        let lo = u8x16_shuffle::<0, 2, 4, 6, 8, 10, 12, 14, 16, 18, 20, 22, 24, 26, 28, 30>(a, b);
        let hi = u8x16_shuffle::<1, 3, 5, 7, 9, 11, 13, 15, 17, 19, 21, 23, 25, 27, 29, 31>(a, b);
        v128_store(output.as_mut_ptr().add(i) as *mut v128, lo);
        v128_store(output.as_mut_ptr().add(half + i) as *mut v128, hi);
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
        let lo = v128_load(input.as_ptr().add(i) as *const v128);
        let hi = v128_load(input.as_ptr().add(half + i) as *const v128);
        let a = u8x16_shuffle::<0, 16, 1, 17, 2, 18, 3, 19, 4, 20, 5, 21, 6, 22, 7, 23>(lo, hi);
        let b =
            u8x16_shuffle::<8, 24, 9, 25, 10, 26, 11, 27, 12, 28, 13, 29, 14, 30, 15, 31>(lo, hi);
        v128_store(output.as_mut_ptr().add(i * 2) as *mut v128, a);
        v128_store(output.as_mut_ptr().add(i * 2 + 16) as *mut v128, b);
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
        let a = v128_load(input.as_ptr().add(i * 4) as *const v128);
        let b = v128_load(input.as_ptr().add(i * 4 + 16) as *const v128);
        let c = v128_load(input.as_ptr().add(i * 4 + 32) as *const v128);
        let d = v128_load(input.as_ptr().add(i * 4 + 48) as *const v128);

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

        v128_store(output.as_mut_ptr().add(i) as *mut v128, r0);
        v128_store(output.as_mut_ptr().add(quarter + i) as *mut v128, r1);
        v128_store(output.as_mut_ptr().add(2 * quarter + i) as *mut v128, r2);
        v128_store(output.as_mut_ptr().add(3 * quarter + i) as *mut v128, r3);
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
        let b0 = v128_load(input.as_ptr().add(i) as *const v128);
        let b1 = v128_load(input.as_ptr().add(quarter + i) as *const v128);
        let b2 = v128_load(input.as_ptr().add(2 * quarter + i) as *const v128);
        let b3 = v128_load(input.as_ptr().add(3 * quarter + i) as *const v128);

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

        v128_store(output.as_mut_ptr().add(i * 4) as *mut v128, r0);
        v128_store(output.as_mut_ptr().add(i * 4 + 16) as *mut v128, r1);
        v128_store(output.as_mut_ptr().add(i * 4 + 32) as *mut v128, r2);
        v128_store(output.as_mut_ptr().add(i * 4 + 48) as *mut v128, r3);
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
        let a = v128_load(input.as_ptr().add(i * 8) as *const v128);
        let b = v128_load(input.as_ptr().add(i * 8 + 16) as *const v128);
        let c = v128_load(input.as_ptr().add(i * 8 + 32) as *const v128);
        let d = v128_load(input.as_ptr().add(i * 8 + 48) as *const v128);
        let e = v128_load(input.as_ptr().add(i * 8 + 64) as *const v128);
        let f = v128_load(input.as_ptr().add(i * 8 + 80) as *const v128);
        let g = v128_load(input.as_ptr().add(i * 8 + 96) as *const v128);
        let h = v128_load(input.as_ptr().add(i * 8 + 112) as *const v128);

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

        v128_store(
            output.as_mut_ptr().add(i) as *mut v128,
            u8x16_shuffle::<0, 1, 2, 3, 4, 5, 6, 7, 16, 17, 18, 19, 20, 21, 22, 23>(t0, t4),
        );
        v128_store(
            output.as_mut_ptr().add(eighth + i) as *mut v128,
            u8x16_shuffle::<0, 1, 2, 3, 4, 5, 6, 7, 16, 17, 18, 19, 20, 21, 22, 23>(t2, t6),
        );
        v128_store(
            output.as_mut_ptr().add(2 * eighth + i) as *mut v128,
            u8x16_shuffle::<8, 9, 10, 11, 12, 13, 14, 15, 24, 25, 26, 27, 28, 29, 30, 31>(t0, t4),
        );
        v128_store(
            output.as_mut_ptr().add(3 * eighth + i) as *mut v128,
            u8x16_shuffle::<8, 9, 10, 11, 12, 13, 14, 15, 24, 25, 26, 27, 28, 29, 30, 31>(t2, t6),
        );
        v128_store(
            output.as_mut_ptr().add(4 * eighth + i) as *mut v128,
            u8x16_shuffle::<0, 1, 2, 3, 4, 5, 6, 7, 16, 17, 18, 19, 20, 21, 22, 23>(t1, t5),
        );
        v128_store(
            output.as_mut_ptr().add(5 * eighth + i) as *mut v128,
            u8x16_shuffle::<0, 1, 2, 3, 4, 5, 6, 7, 16, 17, 18, 19, 20, 21, 22, 23>(t3, t7),
        );
        v128_store(
            output.as_mut_ptr().add(6 * eighth + i) as *mut v128,
            u8x16_shuffle::<8, 9, 10, 11, 12, 13, 14, 15, 24, 25, 26, 27, 28, 29, 30, 31>(t1, t5),
        );
        v128_store(
            output.as_mut_ptr().add(7 * eighth + i) as *mut v128,
            u8x16_shuffle::<8, 9, 10, 11, 12, 13, 14, 15, 24, 25, 26, 27, 28, 29, 30, 31>(t3, t7),
        );
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
        let p0 = v128_load(input.as_ptr().add(i) as *const v128);
        let p1 = v128_load(input.as_ptr().add(eighth + i) as *const v128);
        let p2 = v128_load(input.as_ptr().add(2 * eighth + i) as *const v128);
        let p3 = v128_load(input.as_ptr().add(3 * eighth + i) as *const v128);
        let p4 = v128_load(input.as_ptr().add(4 * eighth + i) as *const v128);
        let p5 = v128_load(input.as_ptr().add(5 * eighth + i) as *const v128);
        let p6 = v128_load(input.as_ptr().add(6 * eighth + i) as *const v128);
        let p7 = v128_load(input.as_ptr().add(7 * eighth + i) as *const v128);

        let s0 = u8x16_shuffle::<0, 16, 1, 17, 2, 18, 3, 19, 4, 20, 5, 21, 6, 22, 7, 23>(p0, p4);
        let s1 =
            u8x16_shuffle::<8, 24, 9, 25, 10, 26, 11, 27, 12, 28, 13, 29, 14, 30, 15, 31>(p0, p4);
        let s2 = u8x16_shuffle::<0, 16, 1, 17, 2, 18, 3, 19, 4, 20, 5, 21, 6, 22, 7, 23>(p1, p5);
        let s3 =
            u8x16_shuffle::<8, 24, 9, 25, 10, 26, 11, 27, 12, 28, 13, 29, 14, 30, 15, 31>(p1, p5);
        let s4 = u8x16_shuffle::<0, 16, 1, 17, 2, 18, 3, 19, 4, 20, 5, 21, 6, 22, 7, 23>(p2, p6);
        let s5 =
            u8x16_shuffle::<8, 24, 9, 25, 10, 26, 11, 27, 12, 28, 13, 29, 14, 30, 15, 31>(p2, p6);
        let s6 = u8x16_shuffle::<0, 16, 1, 17, 2, 18, 3, 19, 4, 20, 5, 21, 6, 22, 7, 23>(p3, p7);
        let s7 =
            u8x16_shuffle::<8, 24, 9, 25, 10, 26, 11, 27, 12, 28, 13, 29, 14, 30, 15, 31>(p3, p7);

        let t0 = u8x16_shuffle::<0, 1, 16, 17, 2, 3, 18, 19, 4, 5, 20, 21, 6, 7, 22, 23>(s0, s4);
        let t1 =
            u8x16_shuffle::<8, 9, 24, 25, 10, 11, 26, 27, 12, 13, 28, 29, 14, 15, 30, 31>(s0, s4);
        let t2 = u8x16_shuffle::<0, 1, 16, 17, 2, 3, 18, 19, 4, 5, 20, 21, 6, 7, 22, 23>(s1, s5);
        let t3 =
            u8x16_shuffle::<8, 9, 24, 25, 10, 11, 26, 27, 12, 13, 28, 29, 14, 15, 30, 31>(s1, s5);
        let t4 = u8x16_shuffle::<0, 1, 16, 17, 2, 3, 18, 19, 4, 5, 20, 21, 6, 7, 22, 23>(s2, s6);
        let t5 =
            u8x16_shuffle::<8, 9, 24, 25, 10, 11, 26, 27, 12, 13, 28, 29, 14, 15, 30, 31>(s2, s6);
        let t6 = u8x16_shuffle::<0, 1, 16, 17, 2, 3, 18, 19, 4, 5, 20, 21, 6, 7, 22, 23>(s3, s7);
        let t7 =
            u8x16_shuffle::<8, 9, 24, 25, 10, 11, 26, 27, 12, 13, 28, 29, 14, 15, 30, 31>(s3, s7);

        v128_store(
            output.as_mut_ptr().add(i * 8) as *mut v128,
            u8x16_shuffle::<0, 1, 2, 3, 16, 17, 18, 19, 4, 5, 6, 7, 20, 21, 22, 23>(t0, t4),
        );
        v128_store(
            output.as_mut_ptr().add(i * 8 + 16) as *mut v128,
            u8x16_shuffle::<8, 9, 10, 11, 24, 25, 26, 27, 12, 13, 14, 15, 28, 29, 30, 31>(t0, t4),
        );
        v128_store(
            output.as_mut_ptr().add(i * 8 + 32) as *mut v128,
            u8x16_shuffle::<0, 1, 2, 3, 16, 17, 18, 19, 4, 5, 6, 7, 20, 21, 22, 23>(t1, t5),
        );
        v128_store(
            output.as_mut_ptr().add(i * 8 + 48) as *mut v128,
            u8x16_shuffle::<8, 9, 10, 11, 24, 25, 26, 27, 12, 13, 14, 15, 28, 29, 30, 31>(t1, t5),
        );
        v128_store(
            output.as_mut_ptr().add(i * 8 + 64) as *mut v128,
            u8x16_shuffle::<0, 1, 2, 3, 16, 17, 18, 19, 4, 5, 6, 7, 20, 21, 22, 23>(t2, t6),
        );
        v128_store(
            output.as_mut_ptr().add(i * 8 + 80) as *mut v128,
            u8x16_shuffle::<8, 9, 10, 11, 24, 25, 26, 27, 12, 13, 14, 15, 28, 29, 30, 31>(t2, t6),
        );
        v128_store(
            output.as_mut_ptr().add(i * 8 + 96) as *mut v128,
            u8x16_shuffle::<0, 1, 2, 3, 16, 17, 18, 19, 4, 5, 6, 7, 20, 21, 22, 23>(t3, t7),
        );
        v128_store(
            output.as_mut_ptr().add(i * 8 + 112) as *mut v128,
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
