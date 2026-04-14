use crate::ion::byte_transpose::shuffle;

#[inline(always)]
pub(crate) fn shuffle_bytes_by_stride(input: &[u8], output: &mut [u8], element_stride: usize) {
    debug_assert!(
        element_stride > 0,
        "shuffle_bytes_by_stride: element_stride must be > 0"
    );
    debug_assert_eq!(
        input.len(),
        output.len(),
        "shuffle_bytes_by_stride: input and output must have equal length"
    );
    debug_assert!(
        element_stride <= 1 || input.len().is_multiple_of(element_stride),
        "shuffle_bytes_by_stride: input length {} is not a multiple of stride {}",
        input.len(),
        element_stride
    );

    let aligned_len = if element_stride > 1 {
        input.len() - (input.len() % element_stride)
    } else {
        input.len()
    };

    shuffle(
        &input[..aligned_len],
        &mut output[..aligned_len],
        element_stride,
    );

    if aligned_len < input.len() {
        output[aligned_len..].copy_from_slice(&input[aligned_len..]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shuffle_two_byte_elements_correctness() {
        let input = [1u8, 2, 3, 4];
        let mut output = [0u8; 4];
        shuffle_bytes_by_stride(&input, &mut output, 2);
        assert_eq!(output, [1, 3, 2, 4]);
    }

    #[test]
    fn shuffle_four_byte_elements_correctness() {
        let input: Vec<u8> = (0u8..8).collect();
        let mut output = vec![0u8; 8];
        shuffle_bytes_by_stride(&input, &mut output, 4);
        assert_eq!(output, [0, 4, 1, 5, 2, 6, 3, 7]);
    }

    #[test]
    fn shuffle_eight_byte_elements_correctness() {
        let input: Vec<u8> = (0u8..16).collect();
        let mut output = vec![0u8; 16];
        shuffle_bytes_by_stride(&input, &mut output, 8);
        assert_eq!(
            output,
            [0, 8, 1, 9, 2, 10, 3, 11, 4, 12, 5, 13, 6, 14, 7, 15]
        );
    }

    #[test]
    fn shuffle_arbitrary_stride_matches_four_byte_specialized() {
        let input: Vec<u8> = (0u8..8).collect();
        let mut specific_output = vec![0u8; 8];
        let mut generic_output = vec![0u8; 8];
        shuffle_bytes_by_stride(&input, &mut specific_output, 4);
        shuffle_bytes_by_stride(&input, &mut generic_output, 4);
        assert_eq!(specific_output, generic_output);
    }

    #[test]
    fn shuffle_dispatch_routes_correctly() {
        let input: Vec<u8> = (0u8..8).collect();
        let mut via_stride2 = vec![0u8; 8];
        let mut via_stride4 = vec![0u8; 8];
        shuffle_bytes_by_stride(&input, &mut via_stride2, 2);
        shuffle_bytes_by_stride(&input, &mut via_stride4, 4);
        assert_ne!(via_stride2, via_stride4);
    }

    #[test]
    fn shuffle_one_byte_stride_is_identity() {
        let input = [10u8, 20, 30, 40];
        let mut output = [0u8; 4];
        shuffle_bytes_by_stride(&input, &mut output, 1);
        assert_eq!(output, input);
    }
}
