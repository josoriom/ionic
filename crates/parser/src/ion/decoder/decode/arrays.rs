use super::*;

#[derive(Debug, Clone, Copy, Default)]
pub struct ArrayRef {
    pub block_id: u32,
    pub element_offset: u64,
    pub element_count: u64,
    pub array_type: u32,
    pub dtype: u8,
    pub array_filter: u8,
    pub encoded_len: u32,
    pub continues_previous_segment: u8,
}

#[derive(Debug, Clone)]
pub struct ArrayGroup {
    pub array_type: u32,
    pub dtype: u8,
    pub array_filter: u8,
    pub refs: Vec<ArrayRef>,
}

#[derive(Clone)]
pub(crate) struct ArrayRefList {
    pub(crate) len: usize,
    pub(crate) inline: [ArrayRef; INLINE_ARRAY_REF_CAP],
    pub(crate) heap: Option<Vec<ArrayRef>>,
}

impl ArrayRefList {
    #[inline]
    pub(crate) fn with_capacity(capacity: usize) -> Self {
        Self {
            len: 0,
            inline: [ArrayRef::default(); INLINE_ARRAY_REF_CAP],
            heap: (capacity > INLINE_ARRAY_REF_CAP).then(|| Vec::with_capacity(capacity)),
        }
    }

    #[inline]
    pub(crate) fn push(&mut self, value: ArrayRef) {
        if let Some(heap) = self.heap.as_mut() {
            heap.push(value);
            self.len = heap.len();
            return;
        }
        self.inline[self.len] = value;
        self.len += 1;
    }

    #[inline]
    pub(crate) fn is_empty(&self) -> bool {
        self.len == 0
    }

    #[inline]
    pub(crate) fn as_slice(&self) -> &[ArrayRef] {
        match self.heap.as_deref() {
            Some(heap) => heap,
            None => &self.inline[..self.len],
        }
    }

    #[inline]
    pub(crate) fn into_vec(self) -> Vec<ArrayRef> {
        self.heap
            .unwrap_or_else(|| self.inline[..self.len].to_vec())
    }
}

#[inline]
pub(crate) fn aref_read_params(array_ref: &ArrayRef) -> (u64, u64, usize) {
    if array_ref.encoded_len > 0 {
        (array_ref.element_offset, array_ref.encoded_len as u64, 1)
    } else {
        (
            array_ref.element_offset,
            array_ref.element_count,
            dtype_stride(array_ref.dtype),
        )
    }
}

#[inline]
pub(crate) fn parse_array_ref(bytes: &[u8]) -> ArrayRef {
    ArrayRef {
        element_offset: u64::from_le_bytes(bytes[0..8].try_into().unwrap()),
        element_count: u64::from_le_bytes(bytes[8..16].try_into().unwrap()),
        block_id: u32::from_le_bytes(bytes[16..20].try_into().unwrap()),
        array_type: u32::from_le_bytes(bytes[20..24].try_into().unwrap()),
        dtype: bytes[24],
        array_filter: bytes[25],
        encoded_len: u32::from_le_bytes(bytes[26..30].try_into().unwrap()),
        continues_previous_segment: bytes[30],
    }
}

#[inline]
pub(crate) fn read_array_refs_from_buffers(
    entries_buf: &[u8],
    arrayrefs_buf: &[u8],
    index: usize,
) -> Option<ArrayRefList> {
    let entry_offset = index.checked_mul(INDEX_ENTRY_BYTES)?;
    let entry_end = entry_offset.checked_add(INDEX_ENTRY_BYTES)?;
    let entry = entries_buf.get(entry_offset..entry_end)?;
    let ref_start = usize::try_from(u64::from_le_bytes(entry[0..8].try_into().unwrap())).ok()?;
    let ref_count = usize::try_from(u64::from_le_bytes(entry[8..16].try_into().unwrap())).ok()?;
    let max_refs = arrayrefs_buf.len() / ARRAY_REF_BYTES;
    if ref_count > max_refs {
        return None;
    }
    let mut refs = ArrayRefList::with_capacity(ref_count);
    for offset in 0..ref_count {
        let pos = ref_start
            .checked_add(offset)?
            .checked_mul(ARRAY_REF_BYTES)?;
        let end = pos.checked_add(ARRAY_REF_BYTES)?;
        refs.push(parse_array_ref(arrayrefs_buf.get(pos..end)?));
    }
    Some(refs)
}

pub(crate) fn array_ref_start_for_item(entries_buf: &[u8], index: usize) -> Option<u64> {
    let entry_offset = index.checked_mul(INDEX_ENTRY_BYTES)?;
    let entry_end = entry_offset.checked_add(INDEX_ENTRY_BYTES)?;
    let entry = entries_buf.get(entry_offset..entry_end)?;
    Some(u64::from_le_bytes(entry[0..8].try_into().unwrap()))
}

pub(crate) fn group_arrays(refs: &[ArrayRef]) -> IonResult<Vec<ArrayGroup>> {
    if refs.is_empty() {
        return Ok(Vec::new());
    }

    if refs[0].continues_previous_segment != 0 {
        return Err("array grouping: first ref must have continues_previous_segment = 0".into());
    }

    let mut groups = Vec::new();
    let mut current_group_refs = Vec::new();
    let mut current_type = refs[0].array_type;
    let mut current_dtype = refs[0].dtype;
    let mut current_filter = refs[0].array_filter;

    for aref in refs {
        if aref.continues_previous_segment != 0 && aref.continues_previous_segment != 1 {
            return Err(format!(
                "array grouping: invalid continues_previous_segment value {}, must be 0 or 1",
                aref.continues_previous_segment
            )
            .into());
        }

        if aref.continues_previous_segment == 0 {
            if !current_group_refs.is_empty() {
                groups.push(ArrayGroup {
                    array_type: current_type,
                    dtype: current_dtype,
                    array_filter: current_filter,
                    refs: current_group_refs,
                });
                current_group_refs = Vec::new();
            }
            current_type = aref.array_type;
            current_dtype = aref.dtype;
            current_filter = aref.array_filter;
        } else if aref.array_type != current_type
            || aref.dtype != current_dtype
            || aref.array_filter != current_filter
        {
            return Err(
                "array grouping: continuation ref has different array_type, dtype, or filter"
                    .into(),
            );
        }

        current_group_refs.push(*aref);
    }

    if !current_group_refs.is_empty() {
        groups.push(ArrayGroup {
            array_type: current_type,
            dtype: current_dtype,
            array_filter: current_filter,
            refs: current_group_refs,
        });
    }

    for group in &groups {
        if group.refs.len() > 1 {
            for aref in &group.refs {
                if aref.encoded_len > 0 {
                    return Err(
                        "array grouping: multi-ref group cannot contain variable-length arrays"
                            .into(),
                    );
                }
            }
        }
    }

    Ok(groups)
}

pub(crate) fn read_group_decoded_bytes(
    group: &ArrayGroup,
    container: &mut BlockReader<DefaultBlockProcessor>,
) -> IonResult<Vec<u8>> {
    let mut decoded = Vec::new();

    for array_ref in &group.refs {
        let (element_offset, count, stride) = aref_read_params(array_ref);
        let raw = container.get_array_bytes_from_block(
            array_ref.block_id,
            element_offset,
            count,
            stride,
            "read_group_decoded_bytes",
        )?;
        let unfiltered = unfilter_array_bytes(raw, group.dtype, group.array_filter)?;
        decoded.extend_from_slice(&unfiltered);
    }

    Ok(decoded)
}

#[inline]
pub(crate) fn dtype_stride(dtype: u8) -> usize {
    match dtype {
        FILE_DTYPE_F64 | FILE_DTYPE_I64 => 8,
        FILE_DTYPE_F32 | FILE_DTYPE_I32 => 4,
        FILE_DTYPE_F16 | FILE_DTYPE_I16 => 2,
        _ => 1,
    }
}

pub(crate) fn unfilter_array_bytes(
    raw: &[u8],
    dtype: u8,
    array_filter: u8,
) -> IonResult<std::borrow::Cow<'_, [u8]>> {
    let pk_id = PackingId::from_byte(array_filter)?;
    match pk_id {
        PackingId::Raw | PackingId::ByteShuffle => Ok(std::borrow::Cow::Borrowed(raw)),
        PackingId::DeltaShuffle => {
            if dtype == FILE_DTYPE_F64 {
                let mut out = Vec::with_capacity(raw.len());
                let mut prev: u64 = 0;
                for chunk in raw.chunks_exact(8) {
                    prev = prev.wrapping_add(u64::from_le_bytes(chunk.try_into().unwrap()));
                    out.extend_from_slice(&prev.to_le_bytes());
                }
                Ok(std::borrow::Cow::Owned(out))
            } else {
                Ok(std::borrow::Cow::Borrowed(raw))
            }
        }
    }
}

pub(crate) fn decode_into(buf: &mut Vec<f64>, raw: &[u8], dtype: u8, array_filter: u8) -> IonResult<()> {
    buf.clear();
    let bytes = unfilter_array_bytes(raw, dtype, array_filter)?;
    match dtype {
        FILE_DTYPE_F64 => {
            buf.reserve(bytes.len() / 8);
            buf.extend(
                bytes
                    .chunks_exact(8)
                    .map(|c| f64::from_le_bytes(c.try_into().unwrap())),
            );
        }
        FILE_DTYPE_F32 => {
            buf.reserve(bytes.len() / 4);
            buf.extend(
                bytes
                    .chunks_exact(4)
                    .map(|c| f32::from_le_bytes(c.try_into().unwrap()) as f64),
            );
        }
        FILE_DTYPE_F16 => {
            buf.reserve(bytes.len() / 2);
            buf.extend(
                bytes
                    .chunks_exact(2)
                    .map(|c| f16_bits_to_f64(u16::from_le_bytes(c.try_into().unwrap()))),
            );
        }
        FILE_DTYPE_I16 => {
            buf.reserve(bytes.len() / 2);
            buf.extend(
                bytes
                    .chunks_exact(2)
                    .map(|c| i16::from_le_bytes(c.try_into().unwrap()) as f64),
            );
        }
        FILE_DTYPE_I32 => {
            buf.reserve(bytes.len() / 4);
            buf.extend(
                bytes
                    .chunks_exact(4)
                    .map(|c| i32::from_le_bytes(c.try_into().unwrap()) as f64),
            );
        }
        FILE_DTYPE_I64 => {
            buf.reserve(bytes.len() / 8);
            buf.extend(
                bytes
                    .chunks_exact(8)
                    .map(|c| i64::from_le_bytes(c.try_into().unwrap()) as f64),
            );
        }
        _ => {
            return Err(IonError::BadDtype {
                dtype,
                kind: "decode array dtype",
            });
        }
    }
    Ok(())
}

#[inline]
pub(crate) fn parse_array_pair(entry_bytes: &[u8], aref_bytes: &[u8]) -> Option<(ArrayRef, ArrayRef)> {
    let ref_start =
        usize::try_from(u64::from_le_bytes(entry_bytes[0..8].try_into().unwrap())).ok()?;
    let ref_count =
        usize::try_from(u64::from_le_bytes(entry_bytes[8..16].try_into().unwrap())).ok()?;
    let start = ref_start.checked_mul(ARRAY_REF_BYTES)?;
    let span = ref_count.checked_mul(ARRAY_REF_BYTES)?;
    let end = start.checked_add(span)?;
    let mut mz_ref = None;
    let mut int_ref = None;
    for bytes in aref_bytes.get(start..end)?.chunks_exact(ARRAY_REF_BYTES) {
        let array_ref = parse_array_ref(bytes);
        match array_ref.array_type {
            ACC_MZ => mz_ref = Some(array_ref),
            ACC_INT => int_ref = Some(array_ref),
            _ => {}
        }
        if let (Some(mz_ref), Some(int_ref)) = (mz_ref, int_ref) {
            return Some((mz_ref, int_ref));
        }
    }
    None
}

#[inline]
pub(crate) fn decode_from_block(
    container: &mut dyn ContainerAccess,
    buf: &mut Vec<f64>,
    array_ref: &ArrayRef,
) -> bool {
    let (element_offset, count, stride) = aref_read_params(array_ref);
    match container.get_array_bytes_from_block(
        array_ref.block_id,
        element_offset,
        count,
        stride,
        "scan",
    ) {
        Ok(raw) => decode_into(buf, raw, array_ref.dtype, array_ref.array_filter).is_ok(),
        Err(_) => false,
    }
}

impl IonReader {
    pub fn spectrum_arrays(&self, index: usize) -> Option<Vec<ArrayRef>> {
        if index >= self.header.spectrum_count as usize {
            return None;
        }
        read_array_refs_from_buffers(&self.spec_entries_buf, &self.spec_array_refs, index)
            .map(ArrayRefList::into_vec)
    }

    pub fn chromatogram_array_refs(&self, index: usize) -> Option<Vec<ArrayRef>> {
        if index >= self.header.chrom_count as usize {
            return None;
        }
        read_array_refs_from_buffers(&self.chrom_entries_buf, &self.chrom_array_refs, index)
            .map(ArrayRefList::into_vec)
    }

    pub fn read_array(
        &mut self,
        array_ref: &ArrayRef,
        out: &mut Vec<f64>,
    ) -> IonResult<()> {
        let (element_offset, count, stride) = aref_read_params(array_ref);
        let raw = self.spec_container.get_array_bytes_from_block(
            array_ref.block_id,
            element_offset,
            count,
            stride,
            "read_array",
        )?;
        decode_into(out, raw, array_ref.dtype, array_ref.array_filter)
    }

    pub fn read_chromatogram_array(
        &mut self,
        array_ref: &ArrayRef,
        out: &mut Vec<f64>,
    ) -> IonResult<()> {
        let container = self
            .chrom_container
            .as_mut()
            .ok_or_else(|| IonError::from("no chromatogram container"))?;
        let (element_offset, count, stride) = aref_read_params(array_ref);
        let raw = container.get_array_bytes_from_block(
            array_ref.block_id,
            element_offset,
            count,
            stride,
            "read_chromatogram_array",
        )?;
        decode_into(out, raw, array_ref.dtype, array_ref.array_filter)
    }

    pub(crate) fn read_group_values(&mut self, group: &ArrayGroup, out: &mut Vec<f64>) -> IonResult<()> {
        let Some((first, rest)) = group.refs.split_first() else {
            out.clear();
            return Ok(());
        };
        self.read_array(first, out)?;
        let mut segment = Vec::new();
        for array_ref in rest {
            self.read_array(array_ref, &mut segment)?;
            out.extend_from_slice(&segment);
        }
        Ok(())
    }

    pub fn read_spectrum_logical_array(
        &mut self,
        spectrum_index: usize,
        array_type: u32,
    ) -> IonResult<Vec<f64>> {
        if spectrum_index >= self.header.spectrum_count as usize {
            return Err("spectrum index out of range".into());
        }

        let Some(array_refs) = read_array_refs_from_buffers(
            &self.spec_entries_buf,
            &self.spec_array_refs,
            spectrum_index,
        ) else {
            return Ok(Vec::new());
        };

        let groups = group_arrays(array_refs.as_slice())?;

        for group in groups {
            if group.array_type != array_type {
                continue;
            }

            let mut values = Vec::new();
            self.read_group_values(&group, &mut values)?;
            return Ok(values);
        }

        Ok(Vec::new())
    }
}
