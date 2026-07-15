mod scalar;

#[cfg(target_arch = "aarch64")]
mod simd_aarch64;
#[cfg(target_arch = "wasm32")]
mod simd_wasm32;
#[cfg(target_arch = "x86_64")]
mod simd_x86_64;

#[cfg(not(any(
    target_arch = "x86_64",
    target_arch = "aarch64",
    target_arch = "wasm32"
)))]
use scalar as platform;
#[cfg(target_arch = "aarch64")]
use simd_aarch64 as platform;
#[cfg(target_arch = "wasm32")]
use simd_wasm32 as platform;
#[cfg(target_arch = "x86_64")]
use simd_x86_64 as platform;

#[inline]
pub(crate) fn shuffle_with_tail(input: &[u8], output: &mut [u8], stride: usize) {
    debug_assert!(stride > 0);
    debug_assert_eq!(input.len(), output.len());
    let aligned = if stride > 1 {
        input.len() - (input.len() % stride)
    } else {
        input.len()
    };
    shuffle(&input[..aligned], &mut output[..aligned], stride);
    if aligned < input.len() {
        output[aligned..].copy_from_slice(&input[aligned..]);
    }
}

#[inline]
pub(crate) fn shuffle(input: &[u8], output: &mut [u8], stride: usize) {
    assert_eq!(
        input.len(),
        output.len(),
        "byte transpose requires equal-length input and output"
    );
    match stride {
        2 => platform::shuffle2(input, output),
        4 => platform::shuffle4(input, output),
        8 => platform::shuffle8(input, output),
        _ => scalar::shuffle_any(input, output, stride),
    }
}

#[inline]
pub(crate) fn unshuffle(input: &[u8], output: &mut [u8], stride: usize) {
    assert_eq!(
        input.len(),
        output.len(),
        "byte transpose requires equal-length input and output"
    );
    match stride {
        2 => platform::unshuffle2(input, output),
        4 => platform::unshuffle4(input, output),
        8 => platform::unshuffle8(input, output),
        _ => scalar::unshuffle_any(input, output, stride),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_input(len: usize) -> Vec<u8> {
        (0..len).map(|i| (i % 251) as u8).collect()
    }

    fn do_shuffle(input: &[u8], stride: usize) -> Vec<u8> {
        let aligned = (input.len() / stride) * stride;
        let mut out = vec![0u8; aligned];
        super::shuffle(&input[..aligned], &mut out, stride);
        out
    }

    fn do_unshuffle(input: &[u8], stride: usize) -> Vec<u8> {
        let mut out = vec![0u8; input.len()];
        super::unshuffle(input, &mut out, stride);
        out
    }

    fn do_scalar_shuffle(input: &[u8], stride: usize) -> Vec<u8> {
        let aligned = (input.len() / stride) * stride;
        let mut out = vec![0u8; aligned];
        scalar::shuffle(&input[..aligned], &mut out, stride);
        out
    }

    fn do_scalar_unshuffle(input: &[u8], stride: usize) -> Vec<u8> {
        let mut out = vec![0u8; input.len()];
        scalar::unshuffle(input, &mut out, stride);
        out
    }

    #[test]
    fn platform_matches_scalar_shuffle() {
        for stride in [2, 4, 8] {
            for len in [0, 16, 32, 64, 128, 256] {
                let input = make_input(len);
                assert_eq!(
                    do_shuffle(&input, stride),
                    do_scalar_shuffle(&input, stride),
                    "shuffle mismatch stride={stride} len={len} arch={}",
                    std::env::consts::ARCH
                );
            }
        }
    }

    #[test]
    fn platform_matches_scalar_unshuffle() {
        for stride in [2, 4, 8] {
            for len in [0, 16, 32, 64, 128, 256] {
                let input = make_input(len);
                let shuffled = do_scalar_shuffle(&input, stride);
                assert_eq!(
                    do_unshuffle(&shuffled, stride),
                    do_scalar_unshuffle(&shuffled, stride),
                    "unshuffle mismatch stride={stride} len={len} arch={}",
                    std::env::consts::ARCH
                );
            }
        }
    }

    #[test]
    fn roundtrip_all_strides_and_sizes() {
        for stride in [2, 4, 8] {
            for len in [0, 2, 16, 17, 32, 64, 100, 128, 256, 10000] {
                let input = make_input(len);
                let aligned = (len / stride) * stride;
                let shuffled = do_shuffle(&input, stride);
                let restored = do_unshuffle(&shuffled, stride);
                assert_eq!(
                    restored,
                    &input[..aligned],
                    "roundtrip failed stride={stride} len={len} arch={}",
                    std::env::consts::ARCH
                );
            }
        }
    }

    #[test]
    fn shuffle2_known_values() {
        let input = vec![0u8, 1, 2, 3, 4, 5, 6, 7];
        let mut out = vec![0u8; 8];
        shuffle(&input, &mut out, 2);
        assert_eq!(out, vec![0, 2, 4, 6, 1, 3, 5, 7]);
    }

    #[test]
    fn shuffle4_known_values() {
        let input = vec![0u8, 1, 2, 3, 4, 5, 6, 7];
        let mut out = vec![0u8; 8];
        shuffle(&input, &mut out, 4);
        assert_eq!(out, vec![0, 4, 1, 5, 2, 6, 3, 7]);
    }

    #[test]
    fn shuffle8_known_values() {
        let input: Vec<u8> = (0u8..16).collect();
        let mut out = vec![0u8; 16];
        shuffle(&input, &mut out, 8);
        assert_eq!(
            out,
            vec![0, 8, 1, 9, 2, 10, 3, 11, 4, 12, 5, 13, 6, 14, 7, 15]
        );
    }

    #[test]
    fn unshuffle2_inverts_shuffle2() {
        let input = vec![0u8, 2, 4, 6, 1, 3, 5, 7];
        let mut out = vec![0u8; 8];
        unshuffle(&input, &mut out, 2);
        assert_eq!(out, vec![0, 1, 2, 3, 4, 5, 6, 7]);
    }

    #[test]
    #[should_panic(expected = "byte transpose requires equal-length input and output")]
    fn shuffle_panics_on_length_mismatch() {
        let input = make_input(8);
        let mut output = vec![0u8; 4];
        shuffle(&input, &mut output, 2);
    }

    #[test]
    #[should_panic(expected = "byte transpose requires equal-length input and output")]
    fn unshuffle_panics_on_length_mismatch() {
        let input = make_input(8);
        let mut output = vec![0u8; 4];
        unshuffle(&input, &mut output, 2);
    }

    #[test]
    fn simd_block_shuffle2_full_block() {
        let input = make_input(32);
        let p = do_shuffle(&input, 2);
        let s = do_scalar_shuffle(&input, 2);
        assert_eq!(
            p,
            s,
            "stride=2 full simd block arch={}",
            std::env::consts::ARCH
        );
    }

    #[test]
    fn simd_block_shuffle4_full_block() {
        let input = make_input(64);
        let p = do_shuffle(&input, 4);
        let s = do_scalar_shuffle(&input, 4);
        assert_eq!(
            p,
            s,
            "stride=4 full simd block arch={}",
            std::env::consts::ARCH
        );
    }

    #[test]
    fn simd_block_shuffle8_full_block() {
        let input = make_input(128);
        let p = do_shuffle(&input, 8);
        let s = do_scalar_shuffle(&input, 8);
        assert_eq!(
            p,
            s,
            "stride=8 full simd block arch={}",
            std::env::consts::ARCH
        );
    }

    #[test]
    fn simd_block_unshuffle2_full_block() {
        let input = make_input(32);
        let shuffled = do_scalar_shuffle(&input, 2);
        assert_eq!(
            do_unshuffle(&shuffled, 2),
            do_scalar_unshuffle(&shuffled, 2),
            "unshuffle stride=2 full simd block arch={}",
            std::env::consts::ARCH
        );
    }

    #[test]
    fn simd_block_unshuffle4_full_block() {
        let input = make_input(64);
        let shuffled = do_scalar_shuffle(&input, 4);
        assert_eq!(
            do_unshuffle(&shuffled, 4),
            do_scalar_unshuffle(&shuffled, 4),
            "unshuffle stride=4 full simd block arch={}",
            std::env::consts::ARCH
        );
    }

    #[test]
    fn simd_block_unshuffle8_full_block() {
        let input = make_input(128);
        let shuffled = do_scalar_shuffle(&input, 8);
        assert_eq!(
            do_unshuffle(&shuffled, 8),
            do_scalar_unshuffle(&shuffled, 8),
            "unshuffle stride=8 full simd block arch={}",
            std::env::consts::ARCH
        );
    }
}
