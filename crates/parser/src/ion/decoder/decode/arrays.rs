use super::*;

#[derive(Debug, Clone, Copy, Default)]
pub struct ArrayAddress {
    pub block_id: u32,
    pub element_offset: u64,
    pub element_count: u64,
    pub array_type: u32,
    pub dtype: u8,
    pub array_filter: u8,
    pub encoded_len: u32,
    pub continues_previous_segment: u8,
    pub array_cv_code: u8,
}

#[derive(Debug, Clone)]
pub struct ArrayGroup {
    pub array_type: u32,
    pub array_cv_code: u8,
    pub dtype: u8,
    pub array_filter: u8,
    pub refs: Vec<ArrayAddress>,
}

#[derive(Clone)]
pub(crate) struct ArrayAddressList {
    pub(crate) len: usize,
    pub(crate) inline: [ArrayAddress; INLINE_ARRAY_ADDRESS_CAP],
    pub(crate) heap: Option<Vec<ArrayAddress>>,
}

impl ArrayAddressList {
    #[inline]
    pub(crate) fn with_capacity(capacity: usize) -> Self {
        Self {
            len: 0,
            inline: [ArrayAddress::default(); INLINE_ARRAY_ADDRESS_CAP],
            heap: (capacity > INLINE_ARRAY_ADDRESS_CAP).then(|| Vec::with_capacity(capacity)),
        }
    }

    #[inline]
    pub(crate) fn push(&mut self, value: ArrayAddress) {
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
    pub(crate) fn as_slice(&self) -> &[ArrayAddress] {
        match self.heap.as_deref() {
            Some(heap) => heap,
            None => &self.inline[..self.len],
        }
    }

    #[inline]
    pub(crate) fn into_vec(self) -> Vec<ArrayAddress> {
        self.heap
            .unwrap_or_else(|| self.inline[..self.len].to_vec())
    }
}

#[inline]
pub(crate) fn address_read_params(array_address: &ArrayAddress) -> (u64, u64, usize) {
    if array_address.encoded_len > 0 {
        (
            array_address.element_offset,
            array_address.encoded_len as u64,
            1,
        )
    } else {
        (
            array_address.element_offset,
            array_address.element_count,
            dtype_stride(array_address.dtype),
        )
    }
}

#[inline]
pub(crate) fn parse_array_address(bytes: &[u8]) -> ArrayAddress {
    ArrayAddress {
        element_offset: u64::from_le_bytes(bytes[0..8].try_into().unwrap()),
        element_count: u64::from_le_bytes(bytes[8..16].try_into().unwrap()),
        block_id: u32::from_le_bytes(bytes[16..20].try_into().unwrap()),
        array_type: u32::from_le_bytes(bytes[20..24].try_into().unwrap()),
        dtype: bytes[24],
        array_filter: bytes[25],
        encoded_len: u32::from_le_bytes(bytes[26..30].try_into().unwrap()),
        continues_previous_segment: bytes[30],
        array_cv_code: bytes[31],
    }
}

#[inline]
pub(crate) fn read_array_addresses_from_buffers(
    entries_buf: &[u8],
    array_addresses_buf: &[u8],
    index: usize,
) -> Option<ArrayAddressList> {
    let entry_offset = index.checked_mul(INDEX_ENTRY_BYTES)?;
    let entry_end = entry_offset.checked_add(INDEX_ENTRY_BYTES)?;
    let entry = entries_buf.get(entry_offset..entry_end)?;
    let ref_start = usize::try_from(u64::from_le_bytes(entry[0..8].try_into().unwrap())).ok()?;
    let address_count =
        usize::try_from(u64::from_le_bytes(entry[8..16].try_into().unwrap())).ok()?;
    let max_refs = array_addresses_buf.len() / ARRAY_ADDRESS_BYTES;
    if address_count > max_refs {
        return None;
    }
    let mut refs = ArrayAddressList::with_capacity(address_count);
    for offset in 0..address_count {
        let pos = ref_start
            .checked_add(offset)?
            .checked_mul(ARRAY_ADDRESS_BYTES)?;
        let end = pos.checked_add(ARRAY_ADDRESS_BYTES)?;
        refs.push(parse_array_address(array_addresses_buf.get(pos..end)?));
    }
    Some(refs)
}

pub(crate) fn group_arrays(refs: &[ArrayAddress]) -> IonResult<Vec<ArrayGroup>> {
    if refs.is_empty() {
        return Ok(Vec::new());
    }

    if refs[0].continues_previous_segment != 0 {
        return Err("array grouping: first ref must have continues_previous_segment = 0".into());
    }

    let mut groups = Vec::new();
    let mut current_group_refs = Vec::new();
    let mut current_type = refs[0].array_type;
    let mut current_cv_code = refs[0].array_cv_code;
    let mut current_dtype = refs[0].dtype;
    let mut current_filter = refs[0].array_filter;

    for address in refs {
        if address.continues_previous_segment != 0 && address.continues_previous_segment != 1 {
            return Err(format!(
                "array grouping: invalid continues_previous_segment value {}, must be 0 or 1",
                address.continues_previous_segment
            )
            .into());
        }

        if address.continues_previous_segment == 0 {
            if !current_group_refs.is_empty() {
                groups.push(ArrayGroup {
                    array_type: current_type,
                    array_cv_code: current_cv_code,
                    dtype: current_dtype,
                    array_filter: current_filter,
                    refs: current_group_refs,
                });
                current_group_refs = Vec::new();
            }
            current_type = address.array_type;
            current_cv_code = address.array_cv_code;
            current_dtype = address.dtype;
            current_filter = address.array_filter;
        } else if address.array_type != current_type
            || address.dtype != current_dtype
            || address.array_filter != current_filter
        {
            return Err(
                "array grouping: continuation ref has different array_type, dtype, or filter"
                    .into(),
            );
        }

        current_group_refs.push(*address);
    }

    if !current_group_refs.is_empty() {
        groups.push(ArrayGroup {
            array_type: current_type,
            array_cv_code: current_cv_code,
            dtype: current_dtype,
            array_filter: current_filter,
            refs: current_group_refs,
        });
    }

    for group in &groups {
        if group.refs.len() > 1 {
            for address in &group.refs {
                if address.encoded_len > 0 {
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
    if let [array_address] = group.refs.as_slice() {
        let (element_offset, count, stride) = address_read_params(array_address);
        let raw = container.get_array_bytes_from_block(
            array_address.block_id,
            element_offset,
            count,
            stride,
            "read_group_decoded_bytes",
        )?;
        let unfiltered = unfilter_array_bytes(raw, group.dtype, group.array_filter)?;
        return Ok(unfiltered.into_owned());
    }

    let mut total = 0usize;
    for array_address in &group.refs {
        let (_, count, stride) = address_read_params(array_address);
        total = total.saturating_add((count as usize).saturating_mul(stride));
    }

    let mut decoded = Vec::new();
    decoded.reserve(total);

    for array_address in &group.refs {
        let (element_offset, count, stride) = address_read_params(array_address);
        let raw = container.get_array_bytes_from_block(
            array_address.block_id,
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
        PackingId::Raw => Ok(std::borrow::Cow::Borrowed(raw)),
        PackingId::ByteShuffle => match crate::ion::packing::Dtype::from_byte(dtype) {
            Ok(
                dtype_enum @ (crate::ion::packing::Dtype::F64 | crate::ion::packing::Dtype::F32),
            ) => {
                let mut out = Vec::new();
                crate::ion::packing::packing_by_id(PackingId::ByteShuffle)
                    .decode(raw, dtype_enum, &mut out)?;
                Ok(std::borrow::Cow::Owned(out))
            }
            _ => Ok(std::borrow::Cow::Borrowed(raw)),
        },
        PackingId::DeltaShuffle => match crate::ion::packing::Dtype::from_byte(dtype) {
            Ok(
                dtype_enum @ (crate::ion::packing::Dtype::F64 | crate::ion::packing::Dtype::F32),
            ) => {
                let mut out = Vec::new();
                crate::ion::packing::packing_by_id(PackingId::DeltaShuffle)
                    .decode(raw, dtype_enum, &mut out)?;
                Ok(std::borrow::Cow::Owned(out))
            }
            _ => Ok(std::borrow::Cow::Borrowed(raw)),
        },
    }
}

pub(crate) fn decode_into(
    buf: &mut Vec<f64>,
    raw: &[u8],
    dtype: u8,
    array_filter: u8,
) -> IonResult<()> {
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

fn collect_entry_array_addresses(
    entry_bytes: &[u8],
    address_bytes: &[u8],
) -> Option<Vec<ArrayAddress>> {
    let ref_start =
        usize::try_from(u64::from_le_bytes(entry_bytes[0..8].try_into().unwrap())).ok()?;
    let address_count =
        usize::try_from(u64::from_le_bytes(entry_bytes[8..16].try_into().unwrap())).ok()?;
    let start = ref_start.checked_mul(ARRAY_ADDRESS_BYTES)?;
    let span = address_count.checked_mul(ARRAY_ADDRESS_BYTES)?;
    let end = start.checked_add(span)?;
    let mut refs = Vec::with_capacity(address_count);
    for bytes in address_bytes
        .get(start..end)?
        .chunks_exact(ARRAY_ADDRESS_BYTES)
    {
        refs.push(parse_array_address(bytes));
    }
    Some(refs)
}

pub(crate) fn read_scan_arrays(
    container: &mut dyn ContainerAccess,
    entry_bytes: &[u8],
    address_bytes: &[u8],
    mz: &mut Vec<f64>,
    intensity: &mut Vec<f64>,
) -> bool {
    mz.clear();
    intensity.clear();
    let Some(refs) = collect_entry_array_addresses(entry_bytes, address_bytes) else {
        return false;
    };
    let Ok(groups) = group_arrays(&refs) else {
        return false;
    };
    let mut segment = Vec::new();
    for group in &groups {
        let target = match group.array_type {
            ACC_MZ => &mut *mz,
            ACC_INT => &mut *intensity,
            _ => continue,
        };
        for array_address in &group.refs {
            let (element_offset, count, stride) = address_read_params(array_address);
            let raw = match container.get_array_bytes_from_block(
                array_address.block_id,
                element_offset,
                count,
                stride,
                "scan",
            ) {
                Ok(raw) => raw,
                Err(_) => return false,
            };
            if decode_into(
                &mut segment,
                raw,
                array_address.dtype,
                array_address.array_filter,
            )
            .is_err()
            {
                return false;
            }
            target.extend_from_slice(&segment);
        }
    }
    mz.len().min(intensity.len()) > 0
}

impl IonReader {
    pub fn spectrum_array_addresses(&self, index: usize) -> Option<Vec<ArrayAddress>> {
        if index >= self.header.spectrum_count as usize {
            return None;
        }
        read_array_addresses_from_buffers(&self.spec_entries_buf, &self.spec_array_addresses, index)
            .map(ArrayAddressList::into_vec)
    }

    pub fn chromatogram_array_addresses(&self, index: usize) -> Option<Vec<ArrayAddress>> {
        if index >= self.header.chrom_count as usize {
            return None;
        }
        read_array_addresses_from_buffers(
            &self.chrom_entries_buf,
            &self.chrom_array_addresses,
            index,
        )
        .map(ArrayAddressList::into_vec)
    }

    pub fn read_spectrum_values(
        &mut self,
        array_address: &ArrayAddress,
        out: &mut Vec<f64>,
    ) -> IonResult<()> {
        let (element_offset, count, stride) = address_read_params(array_address);
        let raw = self.spec_container.get_array_bytes_from_block(
            array_address.block_id,
            element_offset,
            count,
            stride,
            "read_array",
        )?;
        decode_into(out, raw, array_address.dtype, array_address.array_filter)
    }

    pub fn read_spectrum_array(&mut self, array_address: &ArrayAddress) -> IonResult<NumericArray> {
        let (element_offset, count, stride) = address_read_params(array_address);
        let raw = self.spec_container.get_array_bytes_from_block(
            array_address.block_id,
            element_offset,
            count,
            stride,
            "read_spectrum_array",
        )?;
        let values = unfilter_array_bytes(raw, array_address.dtype, array_address.array_filter)?;
        super::to_mzml::decoded_bytes_to_binary_data(&values, array_address.dtype)
    }

    pub fn read_chromatogram_values(
        &mut self,
        array_address: &ArrayAddress,
        out: &mut Vec<f64>,
    ) -> IonResult<()> {
        let container = self
            .chrom_container
            .as_mut()
            .ok_or_else(|| IonError::from("no chromatogram container"))?;
        let (element_offset, count, stride) = address_read_params(array_address);
        let raw = container.get_array_bytes_from_block(
            array_address.block_id,
            element_offset,
            count,
            stride,
            "read_chromatogram_values",
        )?;
        decode_into(out, raw, array_address.dtype, array_address.array_filter)
    }

    pub(crate) fn read_group_values(
        &mut self,
        group: &ArrayGroup,
        out: &mut Vec<f64>,
    ) -> IonResult<()> {
        let Some((first, rest)) = group.refs.split_first() else {
            out.clear();
            return Ok(());
        };
        self.read_spectrum_values(first, out)?;
        let mut window = Vec::new();
        for array_address in rest {
            self.read_spectrum_values(array_address, &mut window)?;
            out.extend_from_slice(&window);
        }
        Ok(())
    }

    pub fn read_spectrum_logical_values(
        &mut self,
        spectrum_index: usize,
        array_type: u32,
    ) -> IonResult<Vec<f64>> {
        if spectrum_index >= self.header.spectrum_count as usize {
            return Err("spectrum index out of range".into());
        }

        let Some(array_addresses) = read_array_addresses_from_buffers(
            &self.spec_entries_buf,
            &self.spec_array_addresses,
            spectrum_index,
        ) else {
            return Ok(Vec::new());
        };

        let groups = group_arrays(array_addresses.as_slice())?;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unfilter_array_bytes_delta_shuffle_f64_matches_cumulative_sum() {
        let deltas: [u64; 4] = [100u64, 5, 3, 10];
        let raw: Vec<u8> = deltas.iter().flat_map(|w| w.to_le_bytes()).collect();

        let unfiltered =
            unfilter_array_bytes(&raw, FILE_DTYPE_F64, PackingId::DeltaShuffle as u8).unwrap();

        let mut expected_prev: u64 = 0;
        let mut expected = Vec::new();
        for delta in deltas {
            expected_prev = expected_prev.wrapping_add(delta);
            expected.extend_from_slice(&expected_prev.to_le_bytes());
        }

        assert_eq!(unfiltered.as_ref(), expected.as_slice());
    }

    #[test]
    fn unfilter_array_bytes_delta_shuffle_rejects_misaligned_f64_input() {
        let raw = [0u8; 7];
        let err = unfilter_array_bytes(&raw, FILE_DTYPE_F64, PackingId::DeltaShuffle as u8)
            .expect_err("7 bytes is not a multiple of the f64 word size");
        assert!(err.contains("not a multiple of the word size"));
    }

    #[test]
    fn unfilter_array_bytes_byte_shuffle_f64_round_trips_71() {
        use crate::ion::packing::{PackingInput, packing_by_id};

        let data: Vec<f64> = (0..600)
            .map(|i| 250.0 + (i as f64) * 0.0137 + ((i * 5 % 11) as f64) * 0.0011)
            .collect();
        let raw: Vec<u8> = data.iter().flat_map(|v| v.to_le_bytes()).collect();

        let mut shuffled = Vec::new();
        packing_by_id(PackingId::ByteShuffle)
            .encode(PackingInput::F64(&data), &mut shuffled)
            .unwrap();

        assert_ne!(
            shuffled, raw,
            "shuffled layout must differ from raw for varying data"
        );

        let unfiltered =
            unfilter_array_bytes(&shuffled, FILE_DTYPE_F64, PackingId::ByteShuffle as u8).unwrap();

        assert_eq!(
            unfiltered.as_ref(),
            raw.as_slice(),
            "decode of ByteShuffle-tagged bytes must unshuffle back to raw"
        );

        let out: Vec<f64> = unfiltered
            .chunks_exact(8)
            .map(|c| f64::from_le_bytes(c.try_into().unwrap()))
            .collect();
        assert_eq!(out.len(), data.len());
        for (a, b) in out.iter().zip(data.iter()) {
            assert_eq!(a.to_bits(), b.to_bits());
        }
    }

    #[test]
    fn unfilter_array_bytes_delta_shuffle_passes_through_unsupported_dtype() {
        let raw = [1u8, 2, 3, 4];
        let unfiltered =
            unfilter_array_bytes(&raw, FILE_DTYPE_I16, PackingId::DeltaShuffle as u8).unwrap();
        assert_eq!(unfiltered.as_ref(), raw.as_slice());
    }
}
