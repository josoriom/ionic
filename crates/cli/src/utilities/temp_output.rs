use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

pub(crate) struct TempOutput {
    path: PathBuf,
    delete_on_drop: bool,
}

impl TempOutput {
    pub(crate) fn new(output_path: &Path) -> Result<Self, String> {
        let output_folder = output_path.parent().unwrap_or_else(|| Path::new("."));
        let output_name = output_path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("ion");
        let process_id = std::process::id();
        let time_id = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|time| time.as_nanos())
            .unwrap_or(0);
        let temp_name = format!(".{output_name}.tmp.{process_id}.{time_id}");
        Ok(Self {
            path: output_folder.join(temp_name),
            delete_on_drop: true,
        })
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn move_to(mut self, output_path: &Path) -> Result<(), String> {
        fs::rename(&self.path, output_path).map_err(|error| {
            format!(
                "cannot move '{}' to '{}': {error}",
                self.path.display(),
                output_path.display()
            )
        })?;
        self.delete_on_drop = false;
        Ok(())
    }
}

impl Drop for TempOutput {
    fn drop(&mut self) {
        if self.delete_on_drop {
            let _ = fs::remove_file(&self.path);
        }
    }
}

pub(crate) fn sweep_orphans(output_path: &Path) -> Result<(), String> {
    let output_folder = output_path.parent().unwrap_or_else(|| Path::new("."));
    let output_name = output_path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| {
            format!(
                "cannot read file name from output path '{}'",
                output_path.display()
            )
        })?;
    let temp_prefix = format!(".{output_name}.tmp.");
    let files = fs::read_dir(output_folder).map_err(|error| {
        format!(
            "cannot read output folder '{}': {error}",
            output_folder.display()
        )
    })?;
    let current_pid = std::process::id();
    for entry in files {
        let entry = entry.map_err(|error| {
            format!(
                "cannot read entry in output folder '{}': {error}",
                output_folder.display()
            )
        })?;
        let file_name = entry.file_name();
        let Some(file_name) = file_name.to_str() else {
            continue;
        };
        let Some(pid_and_time) = file_name.strip_prefix(&temp_prefix) else {
            continue;
        };
        let owner_pid = pid_and_time
            .split('.')
            .next()
            .and_then(|part| part.parse::<u32>().ok());
        let Some(owner_pid) = owner_pid else {
            continue;
        };
        if owner_pid != current_pid && !process_is_running(owner_pid) {
            fs::remove_file(entry.path()).map_err(|error| {
                format!(
                    "cannot remove orphaned temp file '{}': {error}",
                    entry.path().display()
                )
            })?;
        }
    }
    Ok(())
}

#[cfg(unix)]
fn process_is_running(pid: u32) -> bool {
    unsafe extern "C" {
        fn kill(pid: i32, signal: i32) -> i32;
    }
    if unsafe { kill(pid as i32, 0) } == 0 {
        return true;
    }
    std::io::Error::last_os_error().kind() == std::io::ErrorKind::PermissionDenied
}

#[cfg(windows)]
fn process_is_running(pid: u32) -> bool {
    use std::ffi::c_void;

    const SYNCHRONIZE: u32 = 0x0010_0000;
    const WAIT_TIMEOUT: u32 = 0x0000_0102;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn OpenProcess(access: u32, inherit_handle: i32, process_id: u32) -> *mut c_void;
        fn WaitForSingleObject(handle: *mut c_void, milliseconds: u32) -> u32;
        fn CloseHandle(handle: *mut c_void) -> i32;
    }

    let handle = unsafe { OpenProcess(SYNCHRONIZE, 0, pid) };
    if handle.is_null() {
        return std::io::Error::last_os_error().kind() == std::io::ErrorKind::PermissionDenied;
    }
    let state = unsafe { WaitForSingleObject(handle, 0) };
    unsafe {
        CloseHandle(handle);
    }
    state == WAIT_TIMEOUT
}
