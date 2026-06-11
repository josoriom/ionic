use crate::ion::IonResult;
use crate::ion::encoder::utilities::sink::{WriteBytes, SectionChunk};

pub(crate) struct SummaryTable {
    chunk: SectionChunk,
}

impl SummaryTable {
    pub(crate) fn new(chunk: SectionChunk) -> Self {
        Self { chunk }
    }

    pub(crate) fn push(&mut self, record: &[u8]) -> IonResult<()> {
        self.chunk.write(record)
    }

    pub(crate) fn finish(self) -> SectionChunk {
        self.chunk
    }
}

pub(crate) struct IndexTable {
    chunk: SectionChunk,
}

impl IndexTable {
    pub(crate) fn new(chunk: SectionChunk) -> Self {
        Self { chunk }
    }

    pub(crate) fn push(&mut self, first_aref: u64, aref_count: u64) -> IonResult<()> {
        let mut record = [0u8; 16];
        record[0..8].copy_from_slice(&first_aref.to_le_bytes());
        record[8..16].copy_from_slice(&aref_count.to_le_bytes());
        self.chunk.write(&record)
    }

    pub(crate) fn finish(self) -> SectionChunk {
        self.chunk
    }
}

pub(crate) struct ArrayRefTable {
    chunk: SectionChunk,
}

impl ArrayRefTable {
    pub(crate) fn new(chunk: SectionChunk) -> Self {
        Self { chunk }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn push(
        &mut self,
        element_offset: u64,
        element_count: u64,
        block_id: u32,
        array_accession: u32,
        dtype: u8,
        array_filter: u8,
        encoded_len: u32,
        continues_previous_segment: u8,
    ) -> IonResult<()> {
        let mut record = [0u8; 32];
        record[0..8].copy_from_slice(&element_offset.to_le_bytes());
        record[8..16].copy_from_slice(&element_count.to_le_bytes());
        record[16..20].copy_from_slice(&block_id.to_le_bytes());
        record[20..24].copy_from_slice(&array_accession.to_le_bytes());
        record[24] = dtype;
        record[25] = array_filter;
        record[26..30].copy_from_slice(&encoded_len.to_le_bytes());
        record[30] = continues_previous_segment;
        self.chunk.write(&record)
    }

    pub(crate) fn finish(self) -> SectionChunk {
        self.chunk
    }
}

pub(crate) struct SegmentBound {
    pub(crate) array_ref_index: u64,
    pub(crate) low: f64,
    pub(crate) high: f64,
}

pub(crate) struct SegmentBoundsTable {
    chunk: SectionChunk,
}

impl SegmentBoundsTable {
    pub(crate) fn new(chunk: SectionChunk) -> Self {
        Self { chunk }
    }

    pub(crate) fn push(&mut self, bound: SegmentBound) -> IonResult<()> {
        let mut record = [0u8; 24];
        record[0..8].copy_from_slice(&bound.array_ref_index.to_le_bytes());
        record[8..16].copy_from_slice(&bound.low.to_le_bytes());
        record[16..24].copy_from_slice(&bound.high.to_le_bytes());
        self.chunk.write(&record)
    }

    pub(crate) fn finish(self) -> SectionChunk {
        self.chunk
    }
}

pub(crate) fn write_aligned(output: &mut dyn WriteBytes, bytes: &[u8]) -> IonResult<u64> {
    static PAD: [u8; 7] = [0u8; 7];
    let pos = output.position()?;
    let aligned = (pos + 7) & !7;
    if aligned > pos {
        output.write(&PAD[..(aligned - pos) as usize])?;
    }
    output.write(bytes)?;
    Ok(aligned)
}
