#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MetaColumnLayout {
    pub(crate) vi_present: bool,
    pub(crate) voff_present: bool,
    pub(crate) ids_reset: bool,
}

impl MetaColumnLayout {
    pub(crate) fn from_version(version: u16) -> Self {
        match version {
            1 => Self {
                vi_present: true,
                voff_present: true,
                ids_reset: false,
            },
            _ => Self {
                vi_present: false,
                voff_present: false,
                ids_reset: true,
            },
        }
    }

    pub(crate) fn without_ids_reset(self) -> Self {
        Self {
            ids_reset: false,
            ..self
        }
    }
}

#[cfg(test)]
mod tests {
    use super::MetaColumnLayout;

    #[test]
    fn version_one_keeps_both_columns() {
        let layout = MetaColumnLayout::from_version(1);
        assert!(layout.vi_present);
        assert!(layout.voff_present);
    }

    #[test]
    fn version_zero_drops_both_columns() {
        let layout = MetaColumnLayout::from_version(0);
        assert!(!layout.vi_present);
        assert!(!layout.voff_present);
    }

    #[test]
    fn version_one_keeps_group_unique_ids() {
        let layout = MetaColumnLayout::from_version(1);
        assert!(!layout.ids_reset);
    }

    #[test]
    fn version_zero_resets_ids_per_item() {
        let layout = MetaColumnLayout::from_version(0);
        assert!(layout.ids_reset);
    }

    #[test]
    fn without_ids_reset_clears_the_flag_but_keeps_the_rest() {
        let layout = MetaColumnLayout::from_version(0).without_ids_reset();
        assert!(!layout.ids_reset);
        assert!(!layout.vi_present);
        assert!(!layout.voff_present);
    }
}
