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
        unsafe { shuffle2_neon(input, output, half, simd_len) };
    }
    for i in simd_len..half {
        output[i] = input[i * 2];
        output[half + i] = input[i * 2 + 1];
    }
}

#[target_feature(enable = "neon")]
unsafe fn shuffle2_neon(input: &[u8], output: &mut [u8], half: usize, simd_len: usize) {
    use std::arch::aarch64::*;
    let mut i = 0usize;
    while i < simd_len {
        let pair = unsafe { vld2q_u8(input.as_ptr().add(i * 2)) };
        unsafe {
            vst1q_u8(output.as_mut_ptr().add(i), pair.0);
            vst1q_u8(output.as_mut_ptr().add(half + i), pair.1);
        }
        i += 16;
    }
}

fn unshuffle2(input: &[u8], output: &mut [u8]) {
    let half = input.len() / 2;
    let simd_len = half & !15;
    if simd_len > 0 {
        unsafe { unshuffle2_neon(input, output, half, simd_len) };
    }
    for i in simd_len..half {
        output[i * 2] = input[i];
        output[i * 2 + 1] = input[half + i];
    }
}

#[target_feature(enable = "neon")]
unsafe fn unshuffle2_neon(input: &[u8], output: &mut [u8], half: usize, simd_len: usize) {
    use std::arch::aarch64::*;
    let mut i = 0usize;
    while i < simd_len {
        let pair = unsafe {
            uint8x16x2_t(
                vld1q_u8(input.as_ptr().add(i)),
                vld1q_u8(input.as_ptr().add(half + i)),
            )
        };
        unsafe { vst2q_u8(output.as_mut_ptr().add(i * 2), pair) };
        i += 16;
    }
}

fn shuffle4(input: &[u8], output: &mut [u8]) {
    let quarter = input.len() / 4;
    let simd_len = quarter & !15;
    if simd_len > 0 {
        unsafe { shuffle4_neon(input, output, quarter, simd_len) };
    }
    for i in simd_len..quarter {
        output[i] = input[i * 4];
        output[quarter + i] = input[i * 4 + 1];
        output[2 * quarter + i] = input[i * 4 + 2];
        output[3 * quarter + i] = input[i * 4 + 3];
    }
}

#[target_feature(enable = "neon")]
unsafe fn shuffle4_neon(input: &[u8], output: &mut [u8], quarter: usize, simd_len: usize) {
    use std::arch::aarch64::*;
    let mut i = 0usize;
    while i < simd_len {
        let quad = unsafe { vld4q_u8(input.as_ptr().add(i * 4)) };
        unsafe {
            vst1q_u8(output.as_mut_ptr().add(i), quad.0);
            vst1q_u8(output.as_mut_ptr().add(quarter + i), quad.1);
            vst1q_u8(output.as_mut_ptr().add(2 * quarter + i), quad.2);
            vst1q_u8(output.as_mut_ptr().add(3 * quarter + i), quad.3);
        }
        i += 16;
    }
}

fn unshuffle4(input: &[u8], output: &mut [u8]) {
    let quarter = input.len() / 4;
    let simd_len = quarter & !15;
    if simd_len > 0 {
        unsafe { unshuffle4_neon(input, output, quarter, simd_len) };
    }
    for i in simd_len..quarter {
        output[i * 4] = input[i];
        output[i * 4 + 1] = input[quarter + i];
        output[i * 4 + 2] = input[2 * quarter + i];
        output[i * 4 + 3] = input[3 * quarter + i];
    }
}

#[target_feature(enable = "neon")]
unsafe fn unshuffle4_neon(input: &[u8], output: &mut [u8], quarter: usize, simd_len: usize) {
    use std::arch::aarch64::*;
    let mut i = 0usize;
    while i < simd_len {
        let quad = unsafe {
            uint8x16x4_t(
                vld1q_u8(input.as_ptr().add(i)),
                vld1q_u8(input.as_ptr().add(quarter + i)),
                vld1q_u8(input.as_ptr().add(2 * quarter + i)),
                vld1q_u8(input.as_ptr().add(3 * quarter + i)),
            )
        };
        unsafe { vst4q_u8(output.as_mut_ptr().add(i * 4), quad) };
        i += 16;
    }
}

fn shuffle8(input: &[u8], output: &mut [u8]) {
    let eighth = input.len() / 8;
    let simd_len = eighth & !15;
    if simd_len > 0 {
        unsafe { shuffle8_neon(input, output, eighth, simd_len) };
    }
    for i in simd_len..eighth {
        for b in 0..8usize {
            output[b * eighth + i] = input[i * 8 + b];
        }
    }
}

#[target_feature(enable = "neon")]
unsafe fn shuffle8_neon(input: &[u8], output: &mut [u8], eighth: usize, simd_len: usize) {
    use std::arch::aarch64::*;
    let mut i = 0usize;
    while i < simd_len {
        let a = unsafe { vld2q_u8(input.as_ptr().add(i * 8)) };
        let b = unsafe { vld2q_u8(input.as_ptr().add(i * 8 + 32)) };
        let c = unsafe { vld2q_u8(input.as_ptr().add(i * 8 + 64)) };
        let d = unsafe { vld2q_u8(input.as_ptr().add(i * 8 + 96)) };

        let p04 = vuzp1q_u8(a.0, b.0);
        let p26 = vuzp2q_u8(a.0, b.0);
        let p15 = vuzp1q_u8(a.1, b.1);
        let p37 = vuzp2q_u8(a.1, b.1);
        let p04h = vuzp1q_u8(c.0, d.0);
        let p26h = vuzp2q_u8(c.0, d.0);
        let p15h = vuzp1q_u8(c.1, d.1);
        let p37h = vuzp2q_u8(c.1, d.1);

        unsafe {
            vst1q_u8(output.as_mut_ptr().add(i), vuzp1q_u8(p04, p04h));
            vst1q_u8(output.as_mut_ptr().add(eighth + i), vuzp1q_u8(p15, p15h));
            vst1q_u8(
                output.as_mut_ptr().add(2 * eighth + i),
                vuzp1q_u8(p26, p26h),
            );
            vst1q_u8(
                output.as_mut_ptr().add(3 * eighth + i),
                vuzp1q_u8(p37, p37h),
            );
            vst1q_u8(
                output.as_mut_ptr().add(4 * eighth + i),
                vuzp2q_u8(p04, p04h),
            );
            vst1q_u8(
                output.as_mut_ptr().add(5 * eighth + i),
                vuzp2q_u8(p15, p15h),
            );
            vst1q_u8(
                output.as_mut_ptr().add(6 * eighth + i),
                vuzp2q_u8(p26, p26h),
            );
            vst1q_u8(
                output.as_mut_ptr().add(7 * eighth + i),
                vuzp2q_u8(p37, p37h),
            );
        }
        i += 16;
    }
}
fn unshuffle8(input: &[u8], output: &mut [u8]) {
    let eighth = input.len() / 8;
    let simd_len = eighth & !15;
    if simd_len > 0 {
        unsafe { unshuffle8_neon(input, output, eighth, simd_len) };
    }
    for i in simd_len..eighth {
        for b in 0..8usize {
            output[i * 8 + b] = input[b * eighth + i];
        }
    }
}

#[target_feature(enable = "neon")]
unsafe fn unshuffle8_neon(input: &[u8], output: &mut [u8], eighth: usize, simd_len: usize) {
    use std::arch::aarch64::*;
    let mut i = 0usize;
    while i < simd_len {
        let p0 = unsafe { vld1q_u8(input.as_ptr().add(i)) };
        let p1 = unsafe { vld1q_u8(input.as_ptr().add(eighth + i)) };
        let p2 = unsafe { vld1q_u8(input.as_ptr().add(2 * eighth + i)) };
        let p3 = unsafe { vld1q_u8(input.as_ptr().add(3 * eighth + i)) };
        let p4 = unsafe { vld1q_u8(input.as_ptr().add(4 * eighth + i)) };
        let p5 = unsafe { vld1q_u8(input.as_ptr().add(5 * eighth + i)) };
        let p6 = unsafe { vld1q_u8(input.as_ptr().add(6 * eighth + i)) };
        let p7 = unsafe { vld1q_u8(input.as_ptr().add(7 * eighth + i)) };

        let z04 = vzipq_u8(p0, p4);
        let z26 = vzipq_u8(p2, p6);
        let z15 = vzipq_u8(p1, p5);
        let z37 = vzipq_u8(p3, p7);

        let ae = vzipq_u8(z04.0, z26.0);
        let ao = vzipq_u8(z15.0, z37.0);
        let ce = vzipq_u8(z04.1, z26.1);
        let co = vzipq_u8(z15.1, z37.1);

        unsafe {
            vst2q_u8(output.as_mut_ptr().add(i * 8), uint8x16x2_t(ae.0, ao.0));
            vst2q_u8(
                output.as_mut_ptr().add(i * 8 + 32),
                uint8x16x2_t(ae.1, ao.1),
            );
            vst2q_u8(
                output.as_mut_ptr().add(i * 8 + 64),
                uint8x16x2_t(ce.0, co.0),
            );
            vst2q_u8(
                output.as_mut_ptr().add(i * 8 + 96),
                uint8x16x2_t(ce.1, co.1),
            );
        }
        i += 16;
    }
}
