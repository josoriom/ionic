use crate::accessions as acc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Axis {
    Mz,
    Time,
    Mobility,
}

pub(crate) fn axis_of(array_type: u32) -> Option<Axis> {
    match array_type {
        acc::MZ_ARRAY => Some(Axis::Mz),
        acc::TIME_ARRAY => Some(Axis::Time),
        acc::ION_MOBILITY_ARRAY
        | acc::MEAN_ION_MOBILITY_ARRAY
        | acc::RAW_ION_MOBILITY_ARRAY
        | acc::RAW_ION_MOBILITY_DRIFT_TIME_ARRAY => Some(Axis::Mobility),
        _ => None,
    }
}
