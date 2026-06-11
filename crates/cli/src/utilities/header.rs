use std::{fs::File, path::Path};

use ionic::ion::{
    format::{CODEC_NONE, CODEC_ZSTD, FILE_SIGNATURE, FILE_TRAILER, HEADER_SIZE, is_supported},
    get_version_from_header,
};

const RESET: &str = "\x1b[0m";
const GREEN: &str = "\x1b[1;32m";
const RED: &str = "\x1b[1;31m";
const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";

const SPEC_SUMMARY_ROW: usize = 128;
const SEGMENT_BOUND_ROW: usize = 24;

struct Section {
    offset: u64,
    size: u64,
    ok: bool,
}

impl Section {
    fn new(_name: &'static str, offset: u64, size: u64) -> Self {
        Self {
            offset,
            size,
            ok: true,
        }
    }
}

pub(crate) fn check_ion_file(path: &Path) -> Result<(), String> {
    let file = File::open(path).map_err(|e| format!("open failed: {e}"))?;
    let map = unsafe { memmap2::Mmap::map(&file).map_err(|e| format!("mmap failed: {e}"))? };
    let bytes: &[u8] = &map;
    if bytes.len() < HEADER_SIZE {
        return Err(format!(
            "file has {} bytes, expected at least {HEADER_SIZE}",
            bytes.len()
        ));
    }
    let view = HeaderView::new(bytes);
    print_summary(&view);
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
        let sections = build_sections(header, u64_at(header, 400));
        Self {
            bytes,
            header,
            sections,
        }
    }

    fn read_header_u32(&self, offset: usize) -> u32 {
        u32_at(self.header, offset)
    }
    fn read_header_u64(&self, offset: usize) -> u64 {
        u64_at(self.header, offset)
    }

    fn signature_ok(&self) -> bool {
        self.header[0..FILE_SIGNATURE.len()] == FILE_SIGNATURE
    }

    fn header_crc_ok(&self) -> bool {
        self.read_header_u32(1020) == crc32fast::hash(&self.header[0..1020])
    }

    fn trailer_ok(&self) -> bool {
        self.bytes.ends_with(&FILE_TRAILER)
    }

    fn file_size_ok(&self) -> bool {
        self.read_header_u64(400) == self.bytes.len() as u64
    }

    fn spec_dir_fits(&self) -> bool {
        self.read_header_u64(240)
            .checked_mul(32)
            .is_some_and(|bytes| bytes <= self.read_header_u64(216))
    }

    fn chrom_dir_fits(&self) -> bool {
        self.read_header_u64(248)
            .checked_mul(32)
            .is_some_and(|bytes| bytes <= self.read_header_u64(232))
    }

    fn sections_in_bounds(&self) -> bool {
        let trailer_start = self.read_header_u64(400).saturating_sub(8);
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
        let offset = self.read_header_u64(off_at) as usize;
        let len = self.read_header_u64(len_at) as usize;
        let stored = self.read_header_u32(stored_at);
        offset
            .checked_add(len)
            .and_then(|end| self.bytes.get(offset..end))
            .is_some_and(|slice| crc32fast::hash(slice) == stored)
    }

    fn block_dir_crc_ok(
        &self,
        container_off_at: usize,
        container_len_at: usize,
        block_count_at: usize,
        stored_at: usize,
    ) -> bool {
        let container_off = self.read_header_u64(container_off_at);
        let container_len = self.read_header_u64(container_len_at);
        let block_count = self.read_header_u64(block_count_at);
        let stored = self.read_header_u32(stored_at);
        (|| -> Option<bool> {
            let dir_bytes = block_count.checked_mul(32)?;
            if dir_bytes > container_len {
                return Some(false);
            }
            let dir_off = container_off.checked_add(container_len.checked_sub(dir_bytes)?)?;
            let start = usize::try_from(dir_off).ok()?;
            let end = usize::try_from(dir_off.checked_add(dir_bytes)?).ok()?;
            Some(
                self.bytes
                    .get(start..end)
                    .is_some_and(|s| crc32fast::hash(s) == stored),
            )
        })()
        .unwrap_or(false)
    }

    fn spec_summary_buf(&self) -> Option<&[u8]> {
        let off = self.read_header_u64(32) as usize;
        let len = self.read_header_u64(40) as usize;
        off.checked_add(len)
            .and_then(|end| self.bytes.get(off..end))
    }

    fn spec_segment_bounds_plain(&self) -> Option<Vec<u8>> {
        let off = self.read_header_u64(48) as usize;
        let len = self.read_header_u64(56) as usize;
        if len == 0 {
            return None;
        }
        let raw = off
            .checked_add(len)
            .and_then(|end| self.bytes.get(off..end))?;
        let codec = self.header[11];
        if codec == CODEC_NONE {
            return Some(raw.to_vec());
        }
        if codec == CODEC_ZSTD {
            let plain_len = self.read_header_u64(384) as usize;
            return zstd::bulk::decompress(raw, plain_len).ok();
        }
        None
    }

    fn axis_maxes(&self) -> AxisMaxes {
        let mut max_rt: f64 = 0.0;
        let mut max_mz: f64 = 0.0;
        let mut max_x: u32 = 0;
        let mut max_y: u32 = 0;
        let mut max_z: u32 = 0;

        if let Some(buf) = self.spec_summary_buf() {
            for row in buf.chunks_exact(SPEC_SUMMARY_ROW) {
                let rt = f64::from_le_bytes(row[0..8].try_into().unwrap());
                let x = u32::from_le_bytes(row[42..46].try_into().unwrap());
                let y = u32::from_le_bytes(row[46..50].try_into().unwrap());
                let z = u32::from_le_bytes(row[50..54].try_into().unwrap());
                if rt.is_finite() && rt > max_rt {
                    max_rt = rt;
                }
                if x > max_x {
                    max_x = x;
                }
                if y > max_y {
                    max_y = y;
                }
                if z > max_z {
                    max_z = z;
                }
            }
        }

        if let Some(plain) = self.spec_segment_bounds_plain() {
            for row in plain.chunks_exact(SEGMENT_BOUND_ROW) {
                let high = f64::from_le_bytes(row[16..24].try_into().unwrap());
                if high.is_finite() && high > max_mz {
                    max_mz = high;
                }
            }
        }

        AxisMaxes {
            max_rt,
            max_mz,
            max_x,
            max_y,
            max_z,
        }
    }
}

struct AxisMaxes {
    max_rt: f64,
    max_mz: f64,
    max_x: u32,
    max_y: u32,
    max_z: u32,
}

fn build_sections(header: &[u8], total_file_size: u64) -> Vec<Section> {
    let mut sections = vec![
        Section::new("spec_summary", u64_at(header, 32), u64_at(header, 40)),
        Section::new("spec_segment_bounds", u64_at(header, 48), u64_at(header, 56)),
        Section::new("spec_entries", u64_at(header, 64), u64_at(header, 72)),
        Section::new("spec_arrayrefs", u64_at(header, 80), u64_at(header, 88)),
        Section::new("chrom_summary", u64_at(header, 96), u64_at(header, 104)),
        Section::new("chrom_segment_bounds", u64_at(header, 112), u64_at(header, 120)),
        Section::new("chrom_entries", u64_at(header, 128), u64_at(header, 136)),
        Section::new("chrom_arrayrefs", u64_at(header, 144), u64_at(header, 152)),
        Section::new("spec_meta", u64_at(header, 160), u64_at(header, 168)),
        Section::new("chrom_meta", u64_at(header, 176), u64_at(header, 184)),
        Section::new("global_meta", u64_at(header, 192), u64_at(header, 200)),
        Section::new("spec_container", u64_at(header, 208), u64_at(header, 216)),
        Section::new("chrom_container", u64_at(header, 224), u64_at(header, 232)),
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
        let end = sections[l].offset.saturating_add(sections[l].size);
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
    let (version_text, version_ok) = match get_version_from_header(view.bytes) {
        Some(v) => (v.to_string(), Some(is_supported(v))),
        None => ("?".to_string(), Some(false)),
    };
    field("format_version", &version_text, version_ok);
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
    let block_size = view.read_header_u64(16);
    field(
        "block_size",
        &format!(
            "{block_size} bytes  ({:.2} MB)",
            block_size as f64 / (1024.0 * 1024.0)
        ),
        None,
    );
    let segment_size = view.read_header_u64(24);
    field(
        "segment_size",
        &format!(
            "{segment_size} bytes  ({:.2} MB)",
            segment_size as f64 / (1024.0 * 1024.0)
        ),
        None,
    );
    field(
        "spectrum_count",
        &view.read_header_u64(256).to_string(),
        None,
    );
    field("chrom_count", &view.read_header_u64(264).to_string(), None);
    let actual = view.bytes.len() as u64;
    field(
        "total_file_size",
        &format!(
            "{actual} bytes  ({:.2} MB)",
            actual as f64 / (1024.0 * 1024.0)
        ),
        Some(view.file_size_ok() && view.trailer_ok()),
    );

    let ax = view.axis_maxes();
    field("max_mz", &format!("{:.6}", ax.max_mz), None);
    field("max_rt", &format!("{:.6}", ax.max_rt), None);
    if ax.max_x > 0 || ax.max_y > 0 || ax.max_z > 0 {
        field("max_x", &ax.max_x.to_string(), None);
        field("max_y", &ax.max_y.to_string(), None);
        field("max_z", &ax.max_z.to_string(), None);
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
        ("10  A0 spec_summary CRC-32", view.crc_ok(32, 40, 968)),
        ("11  A1 spec_segment_bounds CRC-32", view.crc_ok(48, 56, 972)),
        ("12  A2 spec_entries CRC-32", view.crc_ok(64, 72, 976)),
        ("13  A3 spec_arrayrefs CRC-32", view.crc_ok(80, 88, 980)),
        ("14  B0 chrom_summary CRC-32", view.crc_ok(96, 104, 984)),
        ("15  B1 chrom_segment_bounds CRC-32", view.crc_ok(112, 120, 988)),
        ("16  B2 chrom_entries CRC-32", view.crc_ok(128, 136, 992)),
        ("17  B3 chrom_arrayrefs CRC-32", view.crc_ok(144, 152, 996)),
        (
            "18  spec block directory CRC-32",
            view.block_dir_crc_ok(208, 216, 240, 1000),
        ),
        (
            "19  chrom block directory CRC-32",
            view.block_dir_crc_ok(224, 232, 248, 1004),
        ),
        ("20  C spec_meta CRC-32", view.crc_ok(160, 168, 1008)),
        ("21  D chrom_meta CRC-32", view.crc_ok(176, 184, 1012)),
        ("22  E global_meta CRC-32", view.crc_ok(192, 200, 1016)),
    ];
    for (label, ok) in checks {
        let (color, mark) = if *ok { (GREEN, "✓") } else { (RED, "✗") };
        println!("  {color}{mark}{RESET}  {label}");
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
