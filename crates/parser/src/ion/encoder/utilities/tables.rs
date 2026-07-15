use crate::ion::{
    IonResult,
    encoder::utilities::output::{SectionChunk, WriteBytes},
};

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

    pub(crate) fn push(&mut self, first_address: u64, aentry_count: u64) -> IonResult<()> {
        let mut record = [0u8; 16];
        record[0..8].copy_from_slice(&first_address.to_le_bytes());
        record[8..16].copy_from_slice(&aentry_count.to_le_bytes());
        self.chunk.write(&record)
    }

    pub(crate) fn finish(self) -> SectionChunk {
        self.chunk
    }
}

pub(crate) struct ArrayAddressTable {
    chunk: SectionChunk,
}

impl ArrayAddressTable {
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
        array_cv_code: u8,
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
        record[31] = array_cv_code;
        self.chunk.write(&record)
    }

    pub(crate) fn finish(self) -> SectionChunk {
        self.chunk
    }
}

pub(crate) struct WindowEntry {
    pub(crate) spectrum_index: u32,
    pub(crate) mz_address: u32,
    pub(crate) intensity_address: u32,
}

pub(crate) struct WindowDirectory {
    windows: Vec<Vec<WindowEntry>>,
}

impl WindowDirectory {
    pub(crate) fn new() -> Self {
        Self {
            windows: Vec::new(),
        }
    }

    pub(crate) fn push(&mut self, window: u32, entry: WindowEntry) {
        let window = window as usize;
        if window >= self.windows.len() {
            self.windows.resize_with(window + 1, Vec::new);
        }
        self.windows[window].push(entry);
    }

    pub(crate) fn finish(self) -> IonResult<SectionChunk> {
        let mut chunk = SectionChunk::memory(0);
        let entry_count: usize = self.windows.iter().map(Vec::len).sum();
        if entry_count == 0 {
            return Ok(chunk);
        }
        chunk.write(&(self.windows.len() as u32).to_le_bytes())?;
        chunk.write(&(entry_count as u32).to_le_bytes())?;

        let mut start = 0u32;
        for entries in &self.windows {
            chunk.write(&start.to_le_bytes())?;
            start += entries.len() as u32;
        }
        chunk.write(&start.to_le_bytes())?;

        for entries in &self.windows {
            for entry in entries {
                chunk.write(&entry.spectrum_index.to_le_bytes())?;
            }
        }
        for entries in &self.windows {
            for entry in entries {
                chunk.write(&entry.mz_address.to_le_bytes())?;
            }
        }
        for entries in &self.windows {
            for entry in entries {
                chunk.write(&entry.intensity_address.to_le_bytes())?;
            }
        }
        Ok(chunk)
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
