use crate::ion::IonResult;

pub(crate) const EXTENSION_KIND_A3_SPEC_PIECE_BOUNDS: u32 = 1;
pub(crate) const EXTENSION_KIND_B3_CHROM_PIECE_BOUNDS: u32 = 2;
pub(crate) const EXTENSION_KIND_IMAGING_SPATIAL_INDEX: u32 = 3;
pub(crate) const EXTENSION_KIND_RT_MZ_TILED_INDEX: u32 = 4;
pub(crate) const EXTENSION_RECORD_SIZE: usize = 40;
pub(crate) const PIECE_BOUND_SIZE: usize = 24;

#[derive(Debug, Clone)]
pub(crate) struct ExtensionLocation {
    pub(crate) offset: u64,
    pub(crate) length: u64,
}

#[derive(Debug, Clone)]
pub(crate) struct ExtensionRecord {
    pub(crate) kind: u32,
    pub(crate) offset: u64,
    pub(crate) stored_length: u64,
    pub(crate) plain_length: u64,
    pub(crate) checksum: u32,
}

#[derive(Debug, Clone)]
pub(crate) struct ExtensionTable {
    records: Vec<ExtensionRecord>,
}

impl ExtensionTable {
    pub(crate) fn get(&self, kind: u32) -> Option<&ExtensionRecord> {
        self.records.iter().find(|r| r.kind == kind)
    }

    fn from_records(records: Vec<ExtensionRecord>) -> Self {
        ExtensionTable { records }
    }
}

pub(crate) fn read_extension_table(bytes: &[u8]) -> IonResult<ExtensionTable> {
    if bytes.len() < 8 {
        return Err("extension table: too short for header".into());
    }

    let record_count = u32::from_le_bytes(bytes[0..4].try_into().unwrap()) as usize;

    let reserved = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
    if reserved != 0 {
        return Err("extension table: reserved field must be zero".into());
    }

    let expected_size = match record_count
        .checked_mul(EXTENSION_RECORD_SIZE)
        .and_then(|n| n.checked_add(8))
    {
        Some(size) => size,
        None => return Err("extension table: record count overflow".into()),
    };

    if bytes.len() < expected_size {
        return Err("extension table: buffer too short for records".into());
    }

    let mut records = Vec::with_capacity(record_count);
    for i in 0..record_count {
        let start = 8 + i * EXTENSION_RECORD_SIZE;
        let record = parse_extension_record(&bytes[start..start + EXTENSION_RECORD_SIZE])?;
        records.push(record);
    }

    Ok(ExtensionTable::from_records(records))
}

fn parse_extension_record(bytes: &[u8]) -> IonResult<ExtensionRecord> {
    if bytes.len() < EXTENSION_RECORD_SIZE {
        return Err("extension record: buffer too short".into());
    }

    let kind = u32::from_le_bytes(bytes[0..4].try_into().unwrap());

    let reserved = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
    if reserved != 0 {
        return Err("extension record: reserved field must be zero".into());
    }

    let offset = u64::from_le_bytes(bytes[8..16].try_into().unwrap());
    let stored_length = u64::from_le_bytes(bytes[16..24].try_into().unwrap());
    let plain_length = u64::from_le_bytes(bytes[24..32].try_into().unwrap());
    let checksum = u32::from_le_bytes(bytes[32..36].try_into().unwrap());

    let reserved_end = u32::from_le_bytes(bytes[36..40].try_into().unwrap());
    if reserved_end != 0 {
        return Err("extension record: reserved field at end must be zero".into());
    }

    Ok(ExtensionRecord {
        kind,
        offset,
        stored_length,
        plain_length,
        checksum,
    })
}

pub(crate) fn write_extension_table(records: &[ExtensionRecord]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(8 + records.len() * EXTENSION_RECORD_SIZE);

    buf.extend_from_slice(&(records.len() as u32).to_le_bytes());
    buf.extend_from_slice(&0u32.to_le_bytes());

    for record in records {
        buf.extend_from_slice(&record.kind.to_le_bytes());
        buf.extend_from_slice(&0u32.to_le_bytes());
        buf.extend_from_slice(&record.offset.to_le_bytes());
        buf.extend_from_slice(&record.stored_length.to_le_bytes());
        buf.extend_from_slice(&record.plain_length.to_le_bytes());
        buf.extend_from_slice(&record.checksum.to_le_bytes());
        buf.extend_from_slice(&0u32.to_le_bytes());
    }

    buf
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_then_read_round_trips_records() {
        let records = [
            ExtensionRecord {
                kind: EXTENSION_KIND_A3_SPEC_PIECE_BOUNDS,
                offset: 4096,
                stored_length: 240,
                plain_length: 480,
                checksum: 0xDEAD_BEEF,
            },
            ExtensionRecord {
                kind: 7,
                offset: 0x1_0000_0001,
                stored_length: 0x2_0000_0002,
                plain_length: 0x3_0000_0003,
                checksum: 0x1234_5678,
            },
        ];
        let bytes = write_extension_table(&records);
        let table = read_extension_table(&bytes).unwrap();

        let a3 = table.get(EXTENSION_KIND_A3_SPEC_PIECE_BOUNDS).unwrap();
        assert_eq!(a3.offset, 4096);
        assert_eq!(a3.stored_length, 240);
        assert_eq!(a3.plain_length, 480);
        assert_eq!(a3.checksum, 0xDEAD_BEEF);

        let second = table.get(7).unwrap();
        assert_eq!(second.offset, 0x1_0000_0001);
        assert_eq!(second.stored_length, 0x2_0000_0002);
        assert_eq!(second.plain_length, 0x3_0000_0003);
    }

    #[test]
    fn unknown_kind_is_skipped_not_an_error() {
        let records = [ExtensionRecord {
            kind: 9999,
            offset: 64,
            stored_length: 8,
            plain_length: 8,
            checksum: 1,
        }];
        let bytes = write_extension_table(&records);
        let table = read_extension_table(&bytes).unwrap();
        assert!(table.get(EXTENSION_KIND_A3_SPEC_PIECE_BOUNDS).is_none());
        assert!(table.get(9999).is_some());
    }

    #[test]
    fn empty_table_round_trips() {
        let bytes = write_extension_table(&[]);
        let table = read_extension_table(&bytes).unwrap();
        assert!(table.get(EXTENSION_KIND_A3_SPEC_PIECE_BOUNDS).is_none());
    }

    #[test]
    fn read_rejects_truncated_buffer() {
        let bytes = write_extension_table(&[ExtensionRecord {
            kind: 1,
            offset: 0,
            stored_length: 0,
            plain_length: 0,
            checksum: 0,
        }]);
        assert!(read_extension_table(&bytes[..bytes.len() - 1]).is_err());
    }

    #[test]
    fn reserved_kind_numbers_are_stable() {
        assert_eq!(EXTENSION_KIND_A3_SPEC_PIECE_BOUNDS, 1);
        assert_eq!(EXTENSION_KIND_B3_CHROM_PIECE_BOUNDS, 2);
        assert_eq!(EXTENSION_KIND_IMAGING_SPATIAL_INDEX, 3);
        assert_eq!(EXTENSION_KIND_RT_MZ_TILED_INDEX, 4);
    }
}
