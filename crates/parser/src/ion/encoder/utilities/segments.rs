pub(crate) struct SegmentRange {
    pub(crate) start: usize,
    pub(crate) end: usize,
}

impl SegmentRange {
    pub(crate) fn element_count(&self) -> usize {
        self.end - self.start
    }
}

pub(crate) struct SegmentPlan {
    ranges: Vec<SegmentRange>,
}

impl SegmentPlan {
    pub(crate) fn count(&self) -> usize {
        self.ranges.len()
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = &SegmentRange> {
        self.ranges.iter()
    }
}

pub(crate) fn get_segment_ranges(
    element_count: usize,
    element_bytes: usize,
    segment_size: usize,
) -> SegmentPlan {
    if element_count == 0 {
        return SegmentPlan { ranges: vec![] };
    }

    let target_elements = (segment_size / element_bytes.max(1)).max(1);
    let segment_count = element_count.div_ceil(target_elements);
    let base = element_count / segment_count;
    let extra = element_count % segment_count;

    let mut ranges = Vec::with_capacity(segment_count);
    let mut start = 0;
    for index in 0..segment_count {
        let length = base + if index < extra { 1 } else { 0 };
        let end = start + length;
        ranges.push(SegmentRange { start, end });
        start = end;
    }

    SegmentPlan { ranges }
}

pub(crate) fn allow_split(array_bytes: usize, segment_size: usize, plan: &SegmentPlan) -> bool {
    plan.count() >= 2 && array_bytes >= segment_size * 2
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_array_produces_no_segments() {
        let plan = get_segment_ranges(0, 8, 1024);
        assert_eq!(plan.count(), 0);
    }

    #[test]
    fn single_element_produces_one_segment() {
        let plan = get_segment_ranges(1, 8, 1024);
        assert_eq!(plan.count(), 1);
        let first = plan.iter().next().unwrap();
        assert_eq!(first.start, 0);
        assert_eq!(first.end, 1);
    }

    #[test]
    fn segments_partition_exactly() {
        let plan = get_segment_ranges(1000, 8, 256);
        let mut total = 0;
        for segment in plan.iter() {
            total += segment.element_count();
        }
        assert_eq!(total, 1000);
    }

    #[test]
    fn segments_are_contiguous_with_no_gaps() {
        let plan = get_segment_ranges(1000, 8, 256);
        let mut expected_start = 0;
        for segment in plan.iter() {
            assert_eq!(segment.start, expected_start);
            expected_start = segment.end;
        }
        assert_eq!(expected_start, 1000);
    }

    #[test]
    fn segment_sizes_differ_by_at_most_one() {
        for element_count in [3, 7, 1000, 12345, 50000] {
            let plan = get_segment_ranges(element_count, 8, 256);
            let mut min_size = usize::MAX;
            let mut max_size = 0;
            for segment in plan.iter() {
                let size = segment.element_count();
                min_size = min_size.min(size);
                max_size = max_size.max(size);
            }
            assert!(
                max_size - min_size <= 1,
                "unbalanced segments for {element_count}: min={min_size}, max={max_size}"
            );
        }
    }

    #[test]
    fn every_segment_is_within_target() {
        let segment_size = 256;
        let element_bytes = 8;
        let target_elements = segment_size / element_bytes;
        let plan = get_segment_ranges(1000, element_bytes, segment_size);
        for segment in plan.iter() {
            assert!(segment.element_count() <= target_elements);
        }
    }

    #[test]
    fn allow_split_requires_both_conditions() {
        let plan_single = SegmentPlan {
            ranges: vec![SegmentRange { start: 0, end: 100 }],
        };
        let plan_multiple = SegmentPlan {
            ranges: vec![
                SegmentRange { start: 0, end: 50 },
                SegmentRange { start: 50, end: 100 },
            ],
        };

        assert!(!allow_split(2000, 512, &plan_single), "single segment must not split regardless of size");
        assert!(!allow_split(512, 512, &plan_multiple), "array smaller than two full segments must not split");
        assert!(allow_split(1024, 512, &plan_multiple), "array at least two full segments with multiple plan must split");
    }
}
