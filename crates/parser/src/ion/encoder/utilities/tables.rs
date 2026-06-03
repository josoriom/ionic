use crate::encoder::utilities::le_writers::{write_u64_le, write_u32_le};
use crate::ion::encoder::utilities::encoder_output::EncoderOutput;
use crate::ion::IonResult;

pub(crate) struct SummaryTable {
    bytes: Vec<u8>,
}

impl SummaryTable {
    pub(crate) fn new(item_count_hint: usize, record_size: usize) -> Self {
        Self {
            bytes: Vec::with_capacity(item_count_hint * record_size),
        }
    }

    pub(crate) fn push(&mut self, record: &[u8]) {
        self.bytes.extend_from_slice(record);
    }

    pub(crate) fn finish(self) -> Vec<u8> {
        self.bytes
    }
}

impl Default for SummaryTable {
    fn default() -> Self {
        Self { bytes: Vec::new() }
    }
}

pub(crate) struct IndexTable {
    bytes: Vec<u8>,
}

impl IndexTable {
    pub(crate) fn new(item_count_hint: usize) -> Self {
        Self {
            bytes: Vec::with_capacity(item_count_hint * 16),
        }
    }

    pub(crate) fn push(&mut self, first_aref: u64, aref_count: u64) {
        write_u64_le(&mut self.bytes, first_aref);
        write_u64_le(&mut self.bytes, aref_count);
    }

    pub(crate) fn finish(self) -> Vec<u8> {
        self.bytes
    }
}

impl Default for IndexTable {
    fn default() -> Self {
        Self { bytes: Vec::new() }
    }
}

pub(crate) struct ArrayRefTable {
    bytes: Vec<u8>,
    count: u64,
}

impl ArrayRefTable {
    pub(crate) fn new(item_count_hint: usize) -> Self {
        Self {
            bytes: Vec::with_capacity(item_count_hint * 2 * 32),
            count: 0,
        }
    }

    pub(crate) fn push(
        &mut self,
        element_offset: u64,
        element_count: u64,
        block_id: u32,
        array_accession: u32,
        dtype: u8,
        array_filter: u8,
        encoded_len: u32,
    ) {
        write_u64_le(&mut self.bytes, element_offset);
        write_u64_le(&mut self.bytes, element_count);
        write_u32_le(&mut self.bytes, block_id);
        write_u32_le(&mut self.bytes, array_accession);
        self.bytes.push(dtype);
        self.bytes.push(array_filter);
        write_u32_le(&mut self.bytes, encoded_len);
        self.bytes.extend_from_slice(&[0u8; 2]);
        self.count += 1;
    }

    pub(crate) fn count(&self) -> u64 {
        self.count
    }

    pub(crate) fn finish(self) -> Vec<u8> {
        self.bytes
    }
}

impl Default for ArrayRefTable {
    fn default() -> Self {
        Self {
            bytes: Vec::new(),
            count: 0,
        }
    }
}

pub(crate) fn write_aligned(output: &mut dyn EncoderOutput, bytes: &[u8]) -> IonResult<u64> {
    static PAD: [u8; 7] = [0u8; 7];
    let pos = output.current_byte_position()?;
    let aligned = (pos + 7) & !7;
    if aligned > pos {
        output.write_bytes(&PAD[..(aligned - pos) as usize])?;
    }
    output.write_bytes(bytes)?;
    Ok(aligned)
}
