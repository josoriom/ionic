use std::{fs, path::Path};

const HEADER_SIZE: usize = 1024;
const TRAILER: &[u8; 8] = b"END\0\0\0\0\0";
const RESET: &str = "\x1b[0m";
const GREY: &str = "\x1b[90m";
const GREEN: &str = "\x1b[1;32m";
const RED: &str = "\x1b[1;31m";

// Column widths for the printed table
const COL_OFFSET: usize = 6;
const COL_SIZE: usize = 4;
const COL_NAME: usize = 35;
const COL_KIND: usize = 7;
const COL_VALUE: usize = 36;

#[derive(Clone, Copy)]
enum State {
    Plain,
    Pass,
    Fail,
}

struct Item {
    offset: Option<usize>,
    size: &'static str,
    name: &'static str,
    kind: &'static str,
    text: &'static str,
}

struct Section {
    name: &'static str,
    offset: u64,
    size: u64,
    ok: bool,
}

#[rustfmt::skip]
const ITEMS: &[Item] = &[
    field(0,    "8",   "file_signature",              "u8[8]", "Signature: \"START\\0\\0\\0\"."),
    field(8,    "1",   "endianness_flag",              "u8",    "0 = Little Endian, 1 = Big Endian. Files MUST be little-endian."),
    field(9,    "2",   "format_version",               "u16",   "File format version. 0 = legacy/unversioned, 1 = current format."),
    field(11,   "1",   "compression_codec",            "u8",    "0 = none, 1 = zstd."),
    field(12,   "1",   "compression_level",            "u8",    "Codec level (e.g. 0-21 for zstd)."),
    field(13,   "1",   "default_array_filter",         "u8",    "0 = none, 1 = byte shuffle."),
    field(14,   "2",   "reserved",                     "u8[2]", "Reserved (0)."),
    field(16,   "8",   "target_block_uncompressed_bytes", "u64", "Target uncompressed bytes per block."),
    group("Spectrum sections"),
    field(24,   "8",   "off_spec_filter",              "u64",   "Byte offset to Section A0 (Spectrum Filter Index)."),
    field(32,   "8",   "len_spec_filter",              "u64",   "Byte length of Section A0 (must equal spectrum_count x 128)."),
    field(40,   "8",   "off_spec_entries",             "u64",   "Byte offset to Section A1 (Spectrum Entries)."),
    field(48,   "8",   "len_spec_entries",             "u64",   "On-disk byte length of Section A1."),
    field(56,   "8",   "off_spec_arrayrefs",           "u64",   "Byte offset to Section A2 (Spectrum ArrayRef)."),
    field(64,   "8",   "len_spec_arrayrefs",           "u64",   "On-disk byte length of Section A2."),
    group("Chromatogram sections"),
    field(72,   "8",   "off_chrom_filter",             "u64",   "Byte offset to Section B0 (Chromatogram Filter Index)."),
    field(80,   "8",   "len_chrom_filter",             "u64",   "Byte length of Section B0 (must equal chrom_count x 128)."),
    field(88,   "8",   "off_chrom_entries",            "u64",   "Byte offset to Section B1 (Chromatogram Entries)."),
    field(96,   "8",   "len_chrom_entries",            "u64",   "On-disk byte length of Section B1."),
    field(104,  "8",   "off_chrom_arrayrefs",          "u64",   "Byte offset to Section B2 (Chromatogram ArrayRef)."),
    field(112,  "8",   "len_chrom_arrayrefs",          "u64",   "On-disk byte length of Section B2."),
    group("Metadata sections"),
    field(120,  "8",   "off_spec_meta",                "u64",   "Byte offset to Section C (Spectrum Metadata)."),
    field(128,  "8",   "len_spec_meta",                "u64",   "Compressed (on-disk) byte length of Section C."),
    field(136,  "8",   "off_chrom_meta",               "u64",   "Byte offset to Section D (Chromatogram Metadata)."),
    field(144,  "8",   "len_chrom_meta",               "u64",   "Compressed (on-disk) byte length of Section D."),
    field(152,  "8",   "off_global_meta",              "u64",   "Byte offset to Section E (Global Metadata)."),
    field(160,  "8",   "len_global_meta",              "u64",   "Compressed (on-disk) byte length of Section E."),
    field(168,  "8",   "off_spec_container",           "u64",   "Byte offset to the Spectrum container."),
    field(176,  "8",   "len_spec_container",           "u64",   "Compressed (on-disk) byte length of the Spectrum container."),
    field(184,  "8",   "off_chrom_container",          "u64",   "Byte offset to the Chromatogram container."),
    field(192,  "8",   "len_chrom_container",          "u64",   "Compressed (on-disk) byte length of the Chromatogram container."),
    field(200,  "8",   "spec_block_count",             "u64",   "Number of BlockDirectory entries in the Spectrum container."),
    field(208,  "8",   "chrom_block_count",            "u64",   "Number of BlockDirectory entries in the Chromatogram container."),
    field(216,  "8",   "spectrum_count",               "u64",   "Total number of spectra stored in the file."),
    field(224,  "8",   "chrom_count",                  "u64",   "Total number of chromatograms stored."),
    field(232,  "8",   "spec_meta_count",              "u64",   "Total metadata rows for spectra."),
    field(240,  "8",   "spec_meta_numeric_count",      "u64",   "Total numeric values in spectrum metadata pool."),
    field(248,  "8",   "spec_meta_string_count",       "u64",   "Total string values in spectrum metadata pool."),
    field(256,  "8",   "chrom_meta_count",             "u64",   "Total metadata rows for chromatograms."),
    field(264,  "8",   "chrom_meta_numeric_count",     "u64",   "Total numeric values in chromatogram metadata pool."),
    field(272,  "8",   "chrom_meta_string_count",      "u64",   "Total string values in chromatogram metadata pool."),
    field(280,  "8",   "global_meta_count",            "u64",   "Total metadata rows for global metadata."),
    field(288,  "8",   "global_meta_numeric_count",    "u64",   "Total numeric values in global metadata pool."),
    field(296,  "8",   "global_meta_string_count",     "u64",   "Total string values in global metadata pool."),
    field(304,  "8",   "spec_array_type_count",        "u64",   "Number of array types used by Section A2 array_type."),
    field(312,  "8",   "chrom_array_type_count",       "u64",   "Number of array types used by Section B2 array_type."),
    field(320,  "8",   "spec_meta_uncompressed_bytes", "u64",   "Uncompressed byte size of Section C (Spectrum Metadata)."),
    field(328,  "8",   "chrom_meta_uncompressed_bytes","u64",   "Uncompressed byte size of Section D (Chromatogram Metadata)."),
    field(336,  "8",   "global_meta_uncompressed_bytes","u64",  "Uncompressed byte size of Section E (Global Metadata)."),
    field(344,  "8",   "total_file_size",              "u64",   "Expected total file size in bytes including the trailer."),
    field(352,  "656", "reserved_ext",                 "u8[656]","Reserved (0)."),
    field(1008, "4",   "spec_meta_crc32",              "u32",   "CRC-32 of compressed Section C."),
    field(1012, "4",   "chrom_meta_crc32",             "u32",   "CRC-32 of compressed Section D."),
    field(1016, "4",   "global_meta_crc32",            "u32",   "CRC-32 of compressed Section E."),
    field(1020, "4",   "header_crc32",                 "u32",   "CRC-32 over header bytes 0-1019."),
];

pub(crate) fn print_ion_header(path: &Path) -> Result<(), String> {
    let bytes = fs::read(path).map_err(|e| format!("read failed: {e}"))?;
    if bytes.len() < HEADER_SIZE {
        return Err(format!(
            "header failed: file has {} bytes, expected at least {HEADER_SIZE}",
            bytes.len()
        ));
    }

    let view = HeaderView::new(&bytes);
    print_table(&view);
    Ok(())
}

const fn field(
    offset: usize,
    size: &'static str,
    name: &'static str,
    kind: &'static str,
    text: &'static str,
) -> Item {
    Item {
        offset: Some(offset),
        size,
        name,
        kind,
        text,
    }
}

const fn group(name: &'static str) -> Item {
    Item {
        offset: None,
        size: "",
        name,
        kind: "",
        text: "",
    }
}

struct HeaderView<'a> {
    bytes: &'a [u8],
    header: &'a [u8],
    sections: Vec<Section>,
}

impl<'a> HeaderView<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        let header = &bytes[..HEADER_SIZE];
        let sections = section_checks(
            vec![
                Section::new("spec_filter", u64_at(header, 24), u64_at(header, 32)),
                Section::new("spec_entries", u64_at(header, 40), u64_at(header, 48)),
                Section::new("spec_arrayrefs", u64_at(header, 56), u64_at(header, 64)),
                Section::new("chrom_filter", u64_at(header, 72), u64_at(header, 80)),
                Section::new("chrom_entries", u64_at(header, 88), u64_at(header, 96)),
                Section::new("chrom_arrayrefs", u64_at(header, 104), u64_at(header, 112)),
                Section::new("spec_meta", u64_at(header, 120), u64_at(header, 128)),
                Section::new("chrom_meta", u64_at(header, 136), u64_at(header, 144)),
                Section::new("global_meta", u64_at(header, 152), u64_at(header, 160)),
                Section::new("spec_container", u64_at(header, 168), u64_at(header, 176)),
                Section::new("chrom_container", u64_at(header, 184), u64_at(header, 192)),
            ],
            u64_at(header, 344),
        );
        Self {
            bytes,
            header,
            sections,
        }
    }

    fn value(&self, item: &Item) -> String {
        let offset = match item.offset {
            Some(offset) => offset,
            None => return String::new(),
        };
        match item.name {
            "file_signature" => format!("\"{}\"", bytes_text(&self.header[0..8])),
            "reserved" => hex_bytes(&self.header[14..16]),
            "reserved_ext" => {
                if self.header[352..1008].iter().all(|&byte| byte == 0) {
                    "all zero".to_string()
                } else {
                    "not zero".to_string()
                }
            }
            "total_file_size" => format!("{} (actual {})", self.u64(offset), self.bytes.len()),
            "spec_meta_crc32" => self.crc_value(120, 128, 1008),
            "chrom_meta_crc32" => self.crc_value(136, 144, 1012),
            "global_meta_crc32" => self.crc_value(152, 160, 1016),
            "header_crc32" => format!(
                "0x{:08x} (computed 0x{:08x})",
                self.u32(offset),
                crc32fast::hash(&self.header[0..1020])
            ),
            _ => match item.kind {
                "u8" => self.header[offset].to_string(),
                "u16" => self.u16(offset).to_string(),
                "u32" => format!("0x{:08x}", self.u32(offset)),
                "u64" => self.u64(offset).to_string(),
                _ => String::new(),
            },
        }
    }

    fn state(&self, item: &Item) -> State {
        match item.name {
            "file_signature" => pass(&self.header[0..8] == b"START\0\0\0"),
            "endianness_flag" => pass(self.header[8] == 0),
            "format_version" => pass(self.u16(9) <= 1),
            "compression_codec" => pass(self.header[11] <= 1),
            "compression_level" => pass(self.header[12] <= 22),
            "default_array_filter" => pass(self.header[13] <= 1),
            "reserved" => pass(self.header[14..16].iter().all(|&b| b == 0)),
            "reserved_ext" => pass(self.header[352..1008].iter().all(|&b| b == 0)),
            "total_file_size" => {
                pass(self.u64(344) == self.bytes.len() as u64 && self.bytes.ends_with(TRAILER))
            }
            "header_crc32" => pass(self.u32(1020) == crc32fast::hash(&self.header[0..1020])),
            "spec_meta_crc32" => pass(self.crc_ok(120, 128, 1008)),
            "chrom_meta_crc32" => pass(self.crc_ok(136, 144, 1012)),
            "global_meta_crc32" => pass(self.crc_ok(152, 160, 1016)),
            "len_spec_filter" => {
                pass(self.section_ok("spec_filter") && self.equals_product(32, 216, 128))
            }
            "len_chrom_filter" => {
                pass(self.section_ok("chrom_filter") && self.equals_product(80, 224, 128))
            }
            "len_spec_entries" => {
                pass(self.section_ok("spec_entries") && self.equals_product(48, 216, 16))
            }
            "len_chrom_entries" => {
                pass(self.section_ok("chrom_entries") && self.equals_product(96, 224, 16))
            }
            "len_spec_arrayrefs" => {
                pass(self.section_ok("spec_arrayrefs") && self.u64(64) % 32 == 0)
            }
            "len_chrom_arrayrefs" => {
                pass(self.section_ok("chrom_arrayrefs") && self.u64(112) % 32 == 0)
            }
            "len_spec_container" => {
                pass(self.section_ok("spec_container") && self.fits_directory(200, 176))
            }
            "len_chrom_container" => {
                pass(self.section_ok("chrom_container") && self.fits_directory(208, 192))
            }
            "spec_block_count" => pass(self.fits_directory(200, 176)),
            "chrom_block_count" => pass(self.fits_directory(208, 192)),
            _ => section_for_field(item.name)
                .map(|name| pass(self.section_ok(name)))
                .unwrap_or(State::Plain),
        }
    }

    fn u16(&self, offset: usize) -> u16 {
        u16_at(self.header, offset)
    }

    fn u32(&self, offset: usize) -> u32 {
        u32_at(self.header, offset)
    }

    fn u64(&self, offset: usize) -> u64 {
        u64_at(self.header, offset)
    }

    fn section_ok(&self, name: &str) -> bool {
        self.sections.iter().any(|s| s.name == name && s.ok)
    }

    fn equals_product(&self, value_offset: usize, count_offset: usize, size: u64) -> bool {
        self.u64(count_offset).checked_mul(size) == Some(self.u64(value_offset))
    }

    fn fits_directory(&self, count_offset: usize, size_offset: usize) -> bool {
        self.u64(count_offset)
            .checked_mul(32)
            .is_some_and(|bytes| bytes <= self.u64(size_offset))
    }

    fn crc_value(&self, offset_at: usize, size_at: usize, stored_at: usize) -> String {
        let stored = self.u32(stored_at);
        match file_slice(self.bytes, self.u64(offset_at), self.u64(size_at)) {
            Some(slice) => format!("0x{stored:08x} (computed 0x{:08x})", crc32fast::hash(slice)),
            None => format!("0x{stored:08x} (section out of range)"),
        }
    }

    fn crc_ok(&self, offset_at: usize, size_at: usize, stored_at: usize) -> bool {
        let stored = self.u32(stored_at);
        file_slice(self.bytes, self.u64(offset_at), self.u64(size_at))
            .is_some_and(|slice| crc32fast::hash(slice) == stored)
    }
}

impl Section {
    fn new(name: &'static str, offset: u64, size: u64) -> Self {
        Self {
            name,
            offset,
            size,
            ok: true,
        }
    }
}

fn section_checks(mut sections: Vec<Section>, total_file_size: u64) -> Vec<Section> {
    let trailer_start = total_file_size.saturating_sub(8);
    for section in &mut sections {
        let end = section.offset.checked_add(section.size);
        section.ok = end.is_some_and(|value| value <= trailer_start)
            && (section.size == 0 || section.offset % 8 == 0);
    }

    let mut order: Vec<usize> = (0..sections.len())
        .filter(|&i| sections[i].size > 0)
        .collect();
    order.sort_by_key(|&i| sections[i].offset);

    for pair in order.windows(2) {
        let (left, right) = (pair[0], pair[1]);
        let left_end = sections[left]
            .offset
            .checked_add(sections[left].size)
            .unwrap_or(u64::MAX);
        if left_end > sections[right].offset {
            sections[left].ok = false;
            sections[right].ok = false;
        }
    }

    sections
}

fn section_for_field(name: &str) -> Option<&'static str> {
    match name {
        "off_spec_filter" => Some("spec_filter"),
        "off_spec_entries" => Some("spec_entries"),
        "off_spec_arrayrefs" => Some("spec_arrayrefs"),
        "off_chrom_filter" => Some("chrom_filter"),
        "off_chrom_entries" => Some("chrom_entries"),
        "off_chrom_arrayrefs" => Some("chrom_arrayrefs"),
        "off_spec_meta" | "len_spec_meta" => Some("spec_meta"),
        "off_chrom_meta" | "len_chrom_meta" => Some("chrom_meta"),
        "off_global_meta" | "len_global_meta" => Some("global_meta"),
        "off_spec_container" => Some("spec_container"),
        "off_chrom_container" => Some("chrom_container"),
        _ => None,
    }
}

fn file_slice(bytes: &[u8], offset: u64, size: u64) -> Option<&[u8]> {
    let start = usize::try_from(offset).ok()?;
    let len = usize::try_from(size).ok()?;
    let end = start.checked_add(len)?;
    bytes.get(start..end)
}

fn u16_at(bytes: &[u8], offset: usize) -> u16 {
    let mut out = [0; 2];
    out.copy_from_slice(&bytes[offset..offset + 2]);
    u16::from_le_bytes(out)
}

fn u32_at(bytes: &[u8], offset: usize) -> u32 {
    let mut out = [0; 4];
    out.copy_from_slice(&bytes[offset..offset + 4]);
    u32::from_le_bytes(out)
}

fn u64_at(bytes: &[u8], offset: usize) -> u64 {
    let mut out = [0; 8];
    out.copy_from_slice(&bytes[offset..offset + 8]);
    u64::from_le_bytes(out)
}

fn bytes_text(bytes: &[u8]) -> String {
    let mut text = String::new();
    for &byte in bytes {
        match byte {
            0 => text.push_str("\\0"),
            b' '..=b'~' => text.push(byte as char), // printable ASCII
            _ => text.push_str(&format!("\\x{byte:02x}")),
        }
    }
    text
}

fn hex_bytes(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn pass(ok: bool) -> State {
    if ok { State::Pass } else { State::Fail }
}

fn print_table(view: &HeaderView<'_>) {
    println!(
        "| {} | {} | {} | {} | {} | Description |",
        pad("Offset", COL_OFFSET),
        pad("Size", COL_SIZE),
        pad("Variable Name", COL_NAME),
        pad("Type", COL_KIND),
        pad("Value", COL_VALUE),
    );
    println!(
        "| :{} | :{} | :{} | :{} | :{} | :---------- |",
        "-".repeat(COL_OFFSET - 1),
        "-".repeat(COL_SIZE - 1),
        "-".repeat(COL_NAME - 1),
        "-".repeat(COL_KIND - 1),
        "-".repeat(COL_VALUE - 1),
    );
    for item in ITEMS {
        print_item(view, item);
    }
}

fn print_item(view: &HeaderView<'_>, item: &Item) {
    let offset = item
        .offset
        .map(|value| value.to_string())
        .unwrap_or_default();
    let name = if item.offset.is_some() {
        format!("`{}`", item.name)
    } else {
        format!("**{}**", item.name)
    };
    println!(
        "| {} | {} | {} | {} | {} | {} |",
        pad(&offset, COL_OFFSET),
        pad(item.size, COL_SIZE),
        grey(&pad(&name, COL_NAME)),
        pad(item.kind, COL_KIND),
        state_color(&pad(&view.value(item), COL_VALUE), view.state(item)),
        item.text,
    );
}

fn pad(text: &str, width: usize) -> String {
    format!("{text:<width$}")
}

fn grey(text: &str) -> String {
    format!("{GREY}{text}{RESET}")
}

fn state_color(text: &str, state: State) -> String {
    match state {
        State::Plain => text.to_string(),
        State::Pass => format!("{GREEN}{text}{RESET}"),
        State::Fail => format!("{RED}{text}{RESET}"),
    }
}
