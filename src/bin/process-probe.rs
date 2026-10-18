//! Workspace-only process fixture. This binary never selects installed games.
#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
mod probe {
    use std::ffi::{OsString, c_void};
    use std::os::windows::ffi::OsStringExt;

    type Handle = *mut c_void;

    #[repr(C)]
    struct ProcessEntry32W {
        size: u32,
        usage: u32,
        process_id: u32,
        default_heap_id: usize,
        module_id: u32,
        threads: u32,
        parent_process_id: u32,
        priority: i32,
        flags: u32,
        exe_file: [u16; 260],
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn CreateToolhelp32Snapshot(flags: u32, process_id: u32) -> Handle;
        fn Process32FirstW(snapshot: Handle, entry: *mut ProcessEntry32W) -> i32;
        fn Process32NextW(snapshot: Handle, entry: *mut ProcessEntry32W) -> i32;
        fn OpenProcess(access: u32, inherit: i32, process_id: u32) -> Handle;
        fn QueryFullProcessImageNameW(
            process: Handle,
            flags: u32,
            name: *mut u16,
            size: *mut u32,
        ) -> i32;
        fn CloseHandle(handle: Handle) -> i32;
    }

    struct OwnedHandle(Handle);

    impl Drop for OwnedHandle {
        fn drop(&mut self) {
            // SAFETY: owns one valid Win32 handle.
            unsafe { CloseHandle(self.0) };
        }
    }

    pub fn run() -> Result<(), String> {
        let mut args = std::env::args_os().skip(1);
        let output = match args.next() {
            Some(output) => output,
            None => {
                // A fixture used as the suspended parent gets no args. If it
                // ever executes main, this marker makes the runtime test fail.
                let marker = std::env::current_dir()
                    .map_err(|error| error.to_string())?
                    .join("parent-main-ran.marker");
                std::fs::write(marker, b"parent main unexpectedly executed")
                    .map_err(|error| error.to_string())?;
                return Err("probe requires output file path".into());
            }
        };
        let extra: Vec<_> = args.collect();
        if output == "--stay-alive" || extra.iter().any(|arg| arg == "--stay-alive") {
            std::thread::sleep(std::time::Duration::from_secs(60));
            return Ok(());
        }
        // SAFETY: documented snapshot call, invalid handle checked below.
        let snapshot = unsafe { CreateToolhelp32Snapshot(2, 0) };
        if snapshot as isize == -1 {
            return Err(format!("snapshot: {}", std::io::Error::last_os_error()));
        }
        let snapshot = OwnedHandle(snapshot);
        // SAFETY: POD output structure; size initialized before first API call.
        let mut entry: ProcessEntry32W = unsafe { std::mem::zeroed() };
        entry.size = std::mem::size_of::<ProcessEntry32W>() as u32;
        let mut parent = None;
        // SAFETY: live snapshot and properly sized output.
        let mut has_entry = unsafe { Process32FirstW(snapshot.0, &mut entry) } != 0;
        while has_entry {
            if entry.process_id == std::process::id() {
                parent = Some(entry.parent_process_id);
                break;
            }
            // SAFETY: live snapshot and properly sized output.
            has_entry = unsafe { Process32NextW(snapshot.0, &mut entry) } != 0;
        }
        let parent = parent.ok_or_else(|| "could not locate own parent PID".to_string())?;
        // SAFETY: querying a numeric PID obtained from the process snapshot.
        let parent_handle = unsafe { OpenProcess(0x1000, 0, parent) };
        if parent_handle.is_null() {
            return Err(format!("OpenProcess: {}", std::io::Error::last_os_error()));
        }
        let parent_handle = OwnedHandle(parent_handle);
        let mut image = vec![0u16; 32768];
        let mut size = image.len() as u32;
        // SAFETY: live query handle and writable capacity in UTF-16 units.
        if unsafe { QueryFullProcessImageNameW(parent_handle.0, 0, image.as_mut_ptr(), &mut size) }
            == 0
        {
            return Err(format!(
                "QueryFullProcessImageNameW: {}",
                std::io::Error::last_os_error()
            ));
        }
        let image = OsString::from_wide(&image[..size as usize]);
        std::fs::write(
            output,
            format!(
                "parent-pid={parent}\nparent-image={}\nargs={extra:?}\n",
                image.to_string_lossy()
            ),
        )
        .map_err(|error| format!("probe report: {error}"))
    }
}

fn main() -> std::process::ExitCode {
    #[cfg(windows)]
    if std::env::args_os().nth(1).is_some_and(|arg| {
        arg == "--internal-parent-guardian" || arg == "--internal-launcher-update"
    }) {
        return endfield_direct_launch::entrypoint();
    }
    #[cfg(windows)]
    match probe::run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
    #[cfg(not(windows))]
    {
        eprintln!("Windows-only workspace process fixture");
        std::process::ExitCode::FAILURE
    }
}
