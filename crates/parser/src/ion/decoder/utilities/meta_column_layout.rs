#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MetaColumnLayout {
    pub(crate) ids_reset: bool,
}

impl MetaColumnLayout {
    pub(crate) fn new() -> Self {
        Self { ids_reset: true }
    }

    pub(crate) fn without_ids_reset(self) -> Self {
        Self { ids_reset: false }
    }
}

#[cfg(test)]
mod tests {
    use super::MetaColumnLayout;

    #[test]
    fn new_resets_ids_per_item() {
        let layout = MetaColumnLayout::new();
        assert!(layout.ids_reset);
    }

    #[test]
    fn without_ids_reset_clears_the_flag() {
        let layout = MetaColumnLayout::new().without_ids_reset();
        assert!(!layout.ids_reset);
    }
}
