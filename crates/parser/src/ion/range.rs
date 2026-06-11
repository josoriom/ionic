#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Range {
    pub offset: u64,
    pub length: u64,
}
