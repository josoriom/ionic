#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct WindowRange {
    pub(crate) window_index: u32,
    pub(crate) start: usize,
    pub(crate) end: usize,
}

impl WindowRange {
    pub(crate) fn element_count(&self) -> usize {
        self.end - self.start
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Windowing {
    width: f64,
}

impl Windowing {
    pub(crate) fn new(width: f64) -> Self {
        Self { width }
    }

    pub(crate) fn is_enabled(&self) -> bool {
        self.width > 0.0
    }

    pub(crate) fn window_index(&self, mz: f64) -> u32 {
        (mz / self.width).floor() as u32
    }

    pub(crate) fn split_sorted(&self, count: usize, mz_at: impl Fn(usize) -> f64) -> Vec<WindowRange> {
        let mut ranges = Vec::new();
        let mut start = 0;
        while start < count {
            let window_index = self.window_index(mz_at(start));
            let mut end = start + 1;
            while end < count && self.window_index(mz_at(end)) == window_index {
                end += 1;
            }
            ranges.push(WindowRange {
                window_index,
                start,
                end,
            });
            start = end;
        }
        ranges
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window_indices(ranges: &[WindowRange]) -> Vec<u32> {
        ranges.iter().map(|range| range.window_index).collect()
    }

    #[test]
    fn window_index_uses_floor_of_mz_over_width() {
        let windowing = Windowing::new(50.0);
        assert_eq!(windowing.window_index(0.0), 0);
        assert_eq!(windowing.window_index(49.999), 0);
        assert_eq!(windowing.window_index(50.0), 1);
        assert_eq!(windowing.window_index(401.0), 8);
    }

    #[test]
    fn split_groups_consecutive_values_sharing_a_window() {
        let windowing = Windowing::new(50.0);
        let mz = [10.0, 40.0, 60.0, 90.0, 120.0];
        let ranges = windowing.split_sorted(mz.len(), |i| mz[i]);
        assert_eq!(window_indices(&ranges), vec![0, 1, 2]);
        assert_eq!(ranges[0], WindowRange { window_index: 0, start: 0, end: 2 });
        assert_eq!(ranges[1], WindowRange { window_index: 1, start: 2, end: 4 });
        assert_eq!(ranges[2], WindowRange { window_index: 2, start: 4, end: 5 });
    }

    #[test]
    fn split_skips_empty_windows() {
        let windowing = Windowing::new(50.0);
        let mz = [10.0, 410.0];
        let ranges = windowing.split_sorted(mz.len(), |i| mz[i]);
        assert_eq!(window_indices(&ranges), vec![0, 8]);
    }

    #[test]
    fn split_of_empty_input_is_empty() {
        let windowing = Windowing::new(50.0);
        let ranges = windowing.split_sorted(0, |_| 0.0);
        assert!(ranges.is_empty());
    }

    #[test]
    fn ranges_partition_every_element_in_order() {
        let windowing = Windowing::new(25.0);
        let mz = [1.0, 2.0, 26.0, 27.0, 28.0, 300.0];
        let ranges = windowing.split_sorted(mz.len(), |i| mz[i]);
        let mut next = 0;
        for range in &ranges {
            assert_eq!(range.start, next);
            next = range.end;
        }
        assert_eq!(next, mz.len());
    }

    #[test]
    fn zero_width_is_disabled() {
        assert!(!Windowing::new(0.0).is_enabled());
        assert!(Windowing::new(50.0).is_enabled());
    }
}
