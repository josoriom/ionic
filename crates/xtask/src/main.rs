use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::{env, fs};

use regex::Regex;
use serde_json::Value;

fn main() -> ExitCode {
    let command = env::args().nth(1).unwrap_or_default();
    let result = match command.as_str() {
        "sync" => run_sync(),
        "check" => run_check(),
        "show" => run_show(),
        other => Err(format!(
            "unknown command '{other}', expected: sync | check | show"
        )),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("xtask: {message}");
            ExitCode::FAILURE
        }
    }
}

struct Release {
    package: String,
    current: u16,
    min_supported: u16,
    max_supported: u16,
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

fn load_releases(root: &Path) -> Result<Vec<Release>, String> {
    let path = root.join("config.json");
    let text =
        fs::read_to_string(&path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let value: Value =
        serde_json::from_str(&text).map_err(|e| format!("config.json is not valid json: {e}"))?;
    let entries = value
        .as_array()
        .ok_or("config.json must be an array of releases, newest first")?;
    if entries.is_empty() {
        return Err("config.json must hold at least one release".into());
    }
    let mut releases = Vec::with_capacity(entries.len());
    for entry in entries {
        releases.push(read_release(entry)?);
    }
    validate_history(&releases)?;
    Ok(releases)
}

fn read_release(entry: &Value) -> Result<Release, String> {
    let package = entry
        .get("package")
        .and_then(Value::as_str)
        .ok_or("each release needs a string 'package' version")?
        .to_string();
    let format = entry
        .get("format")
        .ok_or("each release needs a 'format' object")?;
    Ok(Release {
        package,
        current: read_u16(format, "current")?,
        min_supported: read_u16(format, "min_supported")?,
        max_supported: read_u16(format, "max_supported")?,
    })
}

fn read_u16(format: &Value, key: &str) -> Result<u16, String> {
    let number = format
        .get(key)
        .and_then(Value::as_u64)
        .ok_or_else(|| format!("format.{key} must be a number"))?;
    u16::try_from(number).map_err(|_| format!("format.{key} must fit in u16"))
}

fn validate_history(releases: &[Release]) -> Result<(), String> {
    for release in releases {
        if release.min_supported == 0 {
            return Err(format!(
                "release {}: min_supported must be at least 1",
                release.package
            ));
        }
        if release.min_supported > release.current || release.current > release.max_supported {
            return Err(format!(
                "release {}: needs min_supported <= current <= max_supported",
                release.package
            ));
        }
    }
    for pair in releases.windows(2) {
        let newer = parse_semver(&pair[0].package)?;
        let older = parse_semver(&pair[1].package)?;
        if newer <= older {
            return Err(format!(
                "config.json must be newest first: {} is not above {}",
                pair[0].package, pair[1].package
            ));
        }
    }
    Ok(())
}

fn parse_semver(version: &str) -> Result<(u64, u64, u64), String> {
    let mut parts = version.split('.');
    let major = next_number(&mut parts, version)?;
    let minor = next_number(&mut parts, version)?;
    let patch = next_number(&mut parts, version)?;
    if parts.next().is_some() {
        return Err(format!("version '{version}' must be major.minor.patch"));
    }
    Ok((major, minor, patch))
}

fn next_number(parts: &mut std::str::Split<'_, char>, version: &str) -> Result<u64, String> {
    parts
        .next()
        .and_then(|part| part.parse::<u64>().ok())
        .ok_or_else(|| format!("version '{version}' must be major.minor.patch"))
}

fn run_sync() -> Result<(), String> {
    let root = repo_root();
    let releases = load_releases(&root)?;
    let current = &releases[0];
    write_cargo_version(&root, &current.package)?;
    write_generated(&root, current)?;
    println!(
        "synced package {} and format reads {}..={} (writes {})",
        current.package, current.min_supported, current.max_supported, current.current
    );
    Ok(())
}

fn write_cargo_version(root: &Path, package: &str) -> Result<(), String> {
    let path = root.join("Cargo.toml");
    let text =
        fs::read_to_string(&path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let finder = Regex::new(r#"(?m)^version = "[^"]*""#).unwrap();
    if !finder.is_match(&text) {
        return Err("could not find the [workspace.package] version line in Cargo.toml".into());
    }
    let updated = finder.replace(&text, format!(r#"version = "{package}""#).as_str());
    fs::write(&path, updated.as_ref()).map_err(|e| format!("cannot write {}: {e}", path.display()))
}

fn write_generated(root: &Path, current: &Release) -> Result<(), String> {
    let path = root.join("crates/parser/src/ion/version_generated.rs");
    let body = format!(
        "// generated by `make sync` from config.json — do not edit\n\
         pub const CURRENT_VERSION: u16 = {};\n\
         pub const MIN_SUPPORTED_VERSION: u16 = {};\n\
         pub const MAX_SUPPORTED_VERSION: u16 = {};\n",
        current.current, current.min_supported, current.max_supported
    );
    fs::write(&path, body).map_err(|e| format!("cannot write {}: {e}", path.display()))
}

fn run_check() -> Result<(), String> {
    let root = repo_root();
    let releases = load_releases(&root)?;
    let current = &releases[0];

    let cargo_version = read_cargo_version(&root)?;
    if cargo_version != current.package {
        return Err(format!(
            "Cargo.toml version {cargo_version} does not match config.json {} (run `make sync`)",
            current.package
        ));
    }

    let generated = read_generated(&root)?;
    let expected = (
        current.current,
        current.min_supported,
        current.max_supported,
    );
    if generated != expected {
        return Err(format!(
            "version_generated.rs {generated:?} does not match config.json {expected:?} (run `make sync`)"
        ));
    }
    println!("config.json, Cargo.toml and version_generated.rs agree");
    Ok(())
}

fn read_cargo_version(root: &Path) -> Result<String, String> {
    let path = root.join("Cargo.toml");
    let text =
        fs::read_to_string(&path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let finder = Regex::new(r#"(?m)^version = "([^"]*)""#).unwrap();
    finder
        .captures(&text)
        .and_then(|caps| caps.get(1))
        .map(|found| found.as_str().to_string())
        .ok_or_else(|| "could not find the [workspace.package] version line in Cargo.toml".into())
}

fn read_generated(root: &Path) -> Result<(u16, u16, u16), String> {
    let path = root.join("crates/parser/src/ion/version_generated.rs");
    let text =
        fs::read_to_string(&path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let current = read_generated_const(&text, "CURRENT_VERSION")?;
    let min = read_generated_const(&text, "MIN_SUPPORTED_VERSION")?;
    let max = read_generated_const(&text, "MAX_SUPPORTED_VERSION")?;
    Ok((current, min, max))
}

fn read_generated_const(text: &str, name: &str) -> Result<u16, String> {
    let finder = Regex::new(&format!(r"{name}: u16 = (\d+);")).unwrap();
    finder
        .captures(text)
        .and_then(|caps| caps.get(1))
        .and_then(|found| found.as_str().parse::<u16>().ok())
        .ok_or_else(|| format!("could not read {name} from version_generated.rs"))
}

fn run_show() -> Result<(), String> {
    let root = repo_root();
    let releases = load_releases(&root)?;
    let current = &releases[0];
    println!("package: {}", current.package);
    println!(
        "format: writes {}, reads {}..={}",
        current.current, current.min_supported, current.max_supported
    );
    println!("history:");
    for release in &releases {
        println!(
            "  {} -> format reads {}..={} (writes {})",
            release.package, release.min_supported, release.max_supported, release.current
        );
    }
    Ok(())
}
