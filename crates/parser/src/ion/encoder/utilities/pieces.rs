pub(crate) struct PieceRange {
    pub(crate) start: usize,
    pub(crate) end: usize,
}

impl PieceRange {
    pub(crate) fn element_count(&self) -> usize {
        self.end - self.start
    }
}

pub(crate) struct PiecePlan {
    ranges: Vec<PieceRange>,
}

impl PiecePlan {
    pub(crate) fn count(&self) -> usize {
        self.ranges.len()
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = &PieceRange> {
        self.ranges.iter()
    }
}

pub(crate) fn get_piece_ranges(
    element_count: usize,
    element_bytes: usize,
    target_piece_bytes: usize,
) -> PiecePlan {
    if element_count == 0 {
        return PiecePlan {
            ranges: vec![],
        };
    }

    let target_elements = (target_piece_bytes / element_bytes.max(1)).max(1);
    let piece_count = element_count.div_ceil(target_elements);
    let base = element_count / piece_count;
    let extra = element_count % piece_count;

    let mut ranges = Vec::with_capacity(piece_count);
    let mut start = 0;
    for index in 0..piece_count {
        let length = base + if index < extra { 1 } else { 0 };
        let end = start + length;
        ranges.push(PieceRange { start, end });
        start = end;
    }

    PiecePlan { ranges }
}

pub(crate) fn allow_split(
    x_array_bytes: usize,
    min_split_bytes: usize,
    plan: &PiecePlan,
) -> bool {
    x_array_bytes >= min_split_bytes && plan.count() >= 2
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_array_produces_no_pieces() {
        let plan = get_piece_ranges(0, 8, 1024);
        assert_eq!(plan.count(), 0);
    }

    #[test]
    fn single_element_produces_one_piece() {
        let plan = get_piece_ranges(1, 8, 1024);
        assert_eq!(plan.count(), 1);
        let first = plan.iter().next().unwrap();
        assert_eq!(first.start, 0);
        assert_eq!(first.end, 1);
    }

    #[test]
    fn pieces_partition_exactly() {
        let plan = get_piece_ranges(1000, 8, 256);
        let mut total = 0;
        for piece in plan.iter() {
            total += piece.element_count();
        }
        assert_eq!(total, 1000);
    }

    #[test]
    fn pieces_are_contiguous_with_no_gaps() {
        let plan = get_piece_ranges(1000, 8, 256);
        let mut expected_start = 0;
        for piece in plan.iter() {
            assert_eq!(piece.start, expected_start);
            expected_start = piece.end;
        }
        assert_eq!(expected_start, 1000);
    }

    #[test]
    fn piece_sizes_differ_by_at_most_one() {
        for element_count in [3, 7, 1000, 12345, 50000] {
            let plan = get_piece_ranges(element_count, 8, 256);
            let mut min_size = usize::MAX;
            let mut max_size = 0;
            for piece in plan.iter() {
                let size = piece.element_count();
                min_size = min_size.min(size);
                max_size = max_size.max(size);
            }
            assert!(
                max_size - min_size <= 1,
                "unbalanced pieces for {element_count}: min={min_size}, max={max_size}"
            );
        }
    }

    #[test]
    fn every_piece_is_within_target() {
        let target_piece_bytes = 256;
        let element_bytes = 8;
        let target_elements = target_piece_bytes / element_bytes;
        let plan = get_piece_ranges(1000, element_bytes, target_piece_bytes);
        for piece in plan.iter() {
            assert!(piece.element_count() <= target_elements);
        }
    }

    #[test]
    fn allow_split_requires_both_conditions() {
        let plan_single = PiecePlan {
            ranges: vec![PieceRange { start: 0, end: 100 }],
        };
        let plan_multiple = PiecePlan {
            ranges: vec![
                PieceRange { start: 0, end: 50 },
                PieceRange { start: 50, end: 100 },
            ],
        };

        assert!(!allow_split(1000, 512, &plan_single));
        assert!(!allow_split(256, 512, &plan_multiple));
        assert!(allow_split(1000, 512, &plan_multiple));
    }
}
