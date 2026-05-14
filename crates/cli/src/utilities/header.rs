use std::{fs, path::Path};

const HEADER_SIZE: usize = 1024;
const TRAILER: &[u8; 8] = b"END\0\0\0\0\0";
const RESET: &str = "\x1b[0m";
const GREEN: &str = "\x1b[1;32m";
const RED: &str = "\x1b[1;31m";
const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";

struct Section {
    name: &'static str,
    offset: u64,
    size: u64,
    ok: bool,
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

pub(crate) fn check_ion_file(path: &Path) -> Result<(), String> {
    let bytes = fs::read(path).map_err(|e| format!("read failed: {e}"))?;
    if bytes.len() < HEADER_SIZE {
        return Err(format!(
            "file has {} bytes, expected at least {HEADER_SIZE}",
            bytes.len()
        ));
    }
    let view = HeaderView::new(&bytes);
    print_summary(&view);
    println!();
    print_sections(&view);
    println!();
    print_integrity(&view);
    Ok(())
}

struct HeaderView<'a> {
    bytes: &'a [u8],
    header: &'a [u8],
    sections: Vec<Section>,
}

impl<'a> HeaderView<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        let header = &bytes[..HEADER_SIZE];
        let sections = build_sections(header, u64_at(header, 344));
        Self {
            bytes,
            header,
            sections,
        }
    }

    fn h_u16(&self, off: usize) -> u16 {
        u16_at(self.header, off)
    }
    fn h_u32(&self, off: usize) -> u32 {
        u32_at(self.header, off)
    }
    fn h_u64(&self, off: usize) -> u64 {
        u64_at(self.header, off)
    }

    fn signature_ok(&self) -> bool {
        &self.header[0..8] == b"START\0\0\0"
    }

    fn header_crc_ok(&self) -> bool {
        self.h_u32(1020) == crc32fast::hash(&self.header[0..1020])
    }

    fn trailer_ok(&self) -> bool {
        self.bytes.ends_with(TRAILER)
    }

    fn file_size_ok(&self) -> bool {
        self.h_u64(344) == self.bytes.len() as u64
    }

    fn spec_dir_fits(&self) -> bool {
        self.h_u64(200)
            .checked_mul(32)
            .is_some_and(|b| b <= self.h_u64(176))
    }

    fn chrom_dir_fits(&self) -> bool {
        self.h_u64(208)
            .checked_mul(32)
            .is_some_and(|b| b <= self.h_u64(192))
    }

    fn sections_in_bounds(&self) -> bool {
        let trailer_start = self.h_u64(344).saturating_sub(8);
        self.sections.iter().all(|s| {
            s.size == 0
                || s.offset
                    .checked_add(s.size)
                    .is_some_and(|end| end <= trailer_start)
        })
    }

    fn no_overlaps(&self) -> bool {
        let mut sorted: Vec<&Section> = self.sections.iter().filter(|s| s.size > 0).collect();
        sorted.sort_by_key(|s| s.offset);
        sorted
            .windows(2)
            .all(|w| w[0].offset + w[0].size <= w[1].offset)
    }

    fn all_aligned(&self) -> bool {
        self.sections
            .iter()
            .all(|s| s.size == 0 || s.offset % 8 == 0)
    }

    fn crc_ok(&self, off_at: usize, len_at: usize, stored_at: usize) -> bool {
        let off = self.h_u64(off_at) as usize;
        let len = self.h_u64(len_at) as usize;
        let stored = self.h_u32(stored_at);
        off.checked_add(len)
            .and_then(|end| self.bytes.get(off..end))
            .is_some_and(|slice| crc32fast::hash(slice) == stored)
    }
}

fn build_sections(header: &[u8], total_file_size: u64) -> Vec<Section> {
    let mut sections = vec![
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
    ];

    let trailer_start = total_file_size.saturating_sub(8);
    for s in &mut sections {
        s.ok = s
            .offset
            .checked_add(s.size)
            .is_some_and(|end| end <= trailer_start)
            && (s.size == 0 || s.offset % 8 == 0);
    }

    let mut order: Vec<usize> = (0..sections.len())
        .filter(|&i| sections[i].size > 0)
        .collect();
    order.sort_by_key(|&i| sections[i].offset);
    for pair in order.windows(2) {
        let (l, r) = (pair[0], pair[1]);
        let end = sections[l]
            .offset
            .checked_add(sections[l].size)
            .unwrap_or(u64::MAX);
        if end > sections[r].offset {
            sections[l].ok = false;
            sections[r].ok = false;
        }
    }

    sections
}

fn print_summary(view: &HeaderView<'_>) {
    println!("{BOLD}File Summary{RESET}");
    let sig = bytes_text(&view.header[0..8]);
    field(
        "signature",
        &format!("\"{sig}\""),
        Some(view.signature_ok()),
    );
    field("format_version", &view.h_u16(9).to_string(), None);
    let codec = match view.header[11] {
        0 => "none",
        1 => "zstd",
        _ => "unknown",
    };
    field(
        "codec",
        &format!("{codec}  level {}", view.header[12]),
        None,
    );
    let filter = match view.header[13] {
        0 => "none",
        1 => "shuffle",
        _ => "unknown",
    };
    field("array_filter", filter, None);
    let block_size = view.h_u64(16);
    field(
        "block_size",
        &format!(
            "{block_size} bytes  ({:.2} MB)",
            block_size as f64 / (1024.0 * 1024.0)
        ),
        None,
    );
    field("spectrum_count", &view.h_u64(216).to_string(), None);
    field("chrom_count", &view.h_u64(224).to_string(), None);
    let actual = view.bytes.len() as u64;
    field(
        "total_file_size",
        &format!(
            "{actual} bytes  ({:.2} MB)",
            actual as f64 / (1024.0 * 1024.0)
        ),
        Some(view.file_size_ok() && view.trailer_ok()),
    );
}

fn print_sections(view: &HeaderView<'_>) {
    println!("{BOLD}Section Layout{RESET}");
    println!(
        "{DIM}  {:<20} {:>14} {:>14}{RESET}",
        "name", "offset", "size"
    );
    for s in &view.sections {
        let mark = if s.ok {
            format!("{GREEN}✓{RESET}")
        } else {
            format!("{RED}✗{RESET}")
        };
        println!("  {:<20} {:>14} {:>14}  {mark}", s.name, s.offset, s.size);
    }
}

fn print_integrity(view: &HeaderView<'_>) {
    println!("{BOLD}Integrity Checks{RESET}");
    let checks: &[(&str, bool)] = &[
        ("1   file signature", view.signature_ok()),
        ("2   header CRC-32", view.header_crc_ok()),
        ("3   file trailer", view.trailer_ok()),
        ("4   file size matches header", view.file_size_ok()),
        ("5   spec block dir fits container", view.spec_dir_fits()),
        ("6   chrom block dir fits container", view.chrom_dir_fits()),
        ("7   all sections within bounds", view.sections_in_bounds()),
        ("8   no section overlaps", view.no_overlaps()),
        ("9   all offsets 8-byte aligned", view.all_aligned()),
        ("10  spec_meta CRC-32", view.crc_ok(120, 128, 1008)),
        ("11  chrom_meta CRC-32", view.crc_ok(136, 144, 1012)),
        ("12  global_meta CRC-32", view.crc_ok(152, 160, 1016)),
    ];
    for (label, ok) in checks {
        let (color, mark) = if *ok { (GREEN, "PASS") } else { (RED, "FAIL") };
        println!("  {color}[{mark}]{RESET}  {label}");
    }
    let passed = checks.iter().filter(|(_, ok)| *ok).count();
    println!();
    if passed == checks.len() {
        println!(
            "{GREEN}{BOLD}{passed}/{} checks passed{RESET}",
            checks.len()
        );
    } else {
        println!("{RED}{BOLD}{passed}/{} checks passed{RESET}", checks.len());
    }
}

fn field(name: &str, value: &str, status: Option<bool>) {
    let indicator = match status {
        None => String::new(),
        Some(true) => format!("  {GREEN}✓{RESET}"),
        Some(false) => format!("  {RED}✗{RESET}"),
    };
    println!("  {DIM}{name:<20}{RESET}  {value}{indicator}");
}

fn bytes_text(bytes: &[u8]) -> String {
    let mut text = String::new();
    for &byte in bytes {
        match byte {
            0 => text.push_str("\\0"),
            b' '..=b'~' => text.push(byte as char),
            _ => text.push_str(&format!("\\x{byte:02x}")),
        }
    }
    text
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
