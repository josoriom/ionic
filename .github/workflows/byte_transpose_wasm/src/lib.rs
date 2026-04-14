#[path = "../../../../crates/parser/src/ion/byte_transpose/mod.rs"]
mod byte_transpose;

#[cfg(test)]
mod tests {
    use super::byte_transpose;

    #[cfg(target_arch = "wasm32")]
    use wasm_bindgen_test::wasm_bindgen_test;

    fn make_input(len: usize) -> Vec<u8> {
        (0..len).map(|i| (i % 251) as u8).collect()
    }

    fn scalar_shuffle(input: &[u8], stride: usize) -> Vec<u8> {
        let aligned = (input.len() / stride) * stride;
        let count = aligned / stride;
        let mut out = vec![0u8; aligned];
        for byte_pos in 0..stride {
            for i in 0..count {
                out[byte_pos * count + i] = input[i * stride + byte_pos];
            }
        }
        out
    }

    fn scalar_unshuffle(input: &[u8], stride: usize) -> Vec<u8> {
        let count = input.len() / stride;
        let mut out = vec![0u8; input.len()];
        for byte_pos in 0..stride {
            for i in 0..count {
                out[i * stride + byte_pos] = input[byte_pos * count + i];
            }
        }
        out
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test)]
    fn shuffle_matches_scalar() {
        for (stride, len) in [(2usize, 32usize), (4, 64), (8, 128)] {
            let input = make_input(len);
            let mut out = vec![0u8; input.len()];
            byte_transpose::shuffle(&input, &mut out, stride);
            assert_eq!(out, scalar_shuffle(&input, stride));
        }
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test)]
    fn unshuffle_matches_scalar() {
        for (stride, len) in [(2usize, 32usize), (4, 64), (8, 128)] {
            let input = scalar_shuffle(&make_input(len), stride);
            let mut out = vec![0u8; input.len()];
            byte_transpose::unshuffle(&input, &mut out, stride);
            assert_eq!(out, scalar_unshuffle(&input, stride));
        }
    }
}
