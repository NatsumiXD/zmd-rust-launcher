use std::ffi::{OsStr, OsString, c_void};
use std::io;
use std::mem::size_of;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::AsRawHandle;
use std::path::Path;
use std::ptr::null;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

type Handle = *mut c_void;

#[repr(C)]
struct StartupInfoW {
    cb: u32,
    reserved: *mut u16,
    desktop: *mut u16,
    title: *mut u16,
    x: u32,
    y: u32,
    x_size: u32,
    y_size: u32,
    x_count_chars: u32,
    y_count_chars: u32,
    fill_attribute: u32,
    flags: u32,
    show_window: u16,
    reserved2_size: u16,
    reserved2: *mut u8,
    stdin: Handle,
    stdout: Handle,
    stderr: Handle,
}

#[repr(C)]
struct ProcessInformation {
    process: Handle,
    thread: Handle,
    process_id: u32,
    thread_id: u32,
}

#[repr(C)]
struct StartupInfoExW {
    startup: StartupInfoW,
    attribute_list: *mut c_void,
}

const CREATE_SUSPENDED: u32 = 4;
const EXTENDED_STARTUPINFO_PRESENT: u32 = 0x0008_0000;
const PROC_THREAD_ATTRIBUTE_PARENT_PROCESS: usize = 0x0002_0000;
const PROC_THREAD_ATTRIBUTE_HANDLE_LIST: usize = 0x0002_0002;
pub const GUARDIAN_FLAG: &str = "--internal-parent-guardian";

// The mutex prevents the control handler from using a handle concurrently with
// its owner closing it. A usize keeps the static Send without making raw
// process handles generally Send. Only one owned suspended parent is active.
static ACTIVE_PARENT: Mutex<usize> = Mutex::new(0);
static ACTIVE_GAME: Mutex<usize> = Mutex::new(0);
static CANCELLED: AtomicBool = AtomicBool::new(false);
static EXIT_REQUESTED: AtomicBool = AtomicBool::new(false);
#[cfg(test)]
static LAST_CREATED_PARENT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

#[link(name = "kernel32")]
unsafe extern "system" {
    fn CreateProcessW(
        application_name: *const u16,
        command_line: *mut u16,
        process_attributes: *const c_void,
        thread_attributes: *const c_void,
        inherit_handles: i32,
        creation_flags: u32,
        environment: *const c_void,
        current_directory: *const u16,
        startup_info: *const StartupInfoW,
        process_information: *mut ProcessInformation,
    ) -> i32;
    fn CloseHandle(handle: Handle) -> i32;
    fn WaitForSingleObject(handle: Handle, milliseconds: u32) -> u32;
    fn GetExitCodeProcess(process: Handle, exit_code: *mut u32) -> i32;
    fn InitializeProcThreadAttributeList(
        list: *mut c_void,
        count: u32,
        flags: u32,
        size: *mut usize,
    ) -> i32;
    fn UpdateProcThreadAttribute(
        list: *mut c_void,
        flags: u32,
        attribute: usize,
        value: *mut c_void,
        size: usize,
        previous: *mut c_void,
        return_size: *mut usize,
    ) -> i32;
    fn DeleteProcThreadAttributeList(list: *mut c_void);
    fn TerminateProcess(process: Handle, exit_code: u32) -> i32;
    fn SetConsoleCtrlHandler(handler: Option<extern "system" fn(u32) -> i32>, add: i32) -> i32;
    fn GetCurrentProcess() -> Handle;
    fn GetProcessId(process: Handle) -> u32;
    fn GetHandleInformation(handle: Handle, flags: *mut u32) -> i32;
    fn GetFileType(handle: Handle) -> u32;
    fn DuplicateHandle(
        source_process: Handle,
        source: Handle,
        target_process: Handle,
        target: *mut Handle,
        access: u32,
        inherit: i32,
        options: u32,
    ) -> i32;
    fn CreateEventW(
        attributes: *const c_void,
        manual_reset: i32,
        initial_state: i32,
        name: *const u16,
    ) -> Handle;
    fn SetEvent(event: Handle) -> i32;
    fn WaitForMultipleObjects(
        count: u32,
        handles: *const Handle,
        wait_all: i32,
        milliseconds: u32,
    ) -> u32;
}

struct OwnedHandle(Handle);

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        // SAFETY: constructed only from successful CreateProcessW handles and
        // owned once. CloseHandle does not require another allocation to live.
        unsafe { CloseHandle(self.0) };
    }
}

pub struct Child {
    process: OwnedHandle,
    id: u32,
    // The guardian owns independent parent/image references. Dropping these
    // visible-launcher handles never terminates a successfully handed-off game.
    parent: Option<SuspendedParent>,
    guardian: Option<OwnedHandle>,
    _control: Option<ConsoleControl>,
}

impl Drop for Child {
    fn drop(&mut self) {
        // Clear the borrowed handle under the same lock used by the callback,
        // before Rust drops and closes the process handle below.
        let mut active = ACTIVE_GAME.lock().unwrap_or_else(|p| p.into_inner());
        if *active == self.process.0 as usize {
            *active = 0;
        }
    }
}

struct ConsoleControl;

impl ConsoleControl {
    fn install() -> Result<Self, String> {
        CANCELLED.store(false, Ordering::SeqCst);
        EXIT_REQUESTED.store(false, Ordering::SeqCst);
        // SAFETY: handler has static lifetime and the required system ABI.
        if unsafe { SetConsoleCtrlHandler(Some(console_control_handler), 1) } == 0 {
            return Err(format!(
                "failed to install game cleanup control handler: {}",
                io::Error::last_os_error()
            ));
        }
        Ok(Self)
    }
}

impl Drop for ConsoleControl {
    fn drop(&mut self) {
        // SAFETY: removes exactly the handler registered by this guard.
        unsafe { SetConsoleCtrlHandler(Some(console_control_handler), 0) };
    }
}

extern "system" fn console_control_handler(event: u32) -> i32 {
    if matches!(event, 0 | 1 | 2 | 5 | 6) {
        EXIT_REQUESTED.store(true, Ordering::SeqCst);
        let game = ACTIVE_GAME.lock().unwrap_or_else(|p| p.into_inner());
        // Launcher exit never terminates a running game. A detached guardian
        // retains its official parent independently of this console.
        let mut active = ACTIVE_PARENT
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if *game == 0 && *active != 0 {
            // SAFETY: the mutex prevents the owner from closing/reusing this
            // handle. It always identifies a suspended process created by us.
            unsafe {
                TerminateProcess(*active as Handle, 1);
                WaitForSingleObject(*active as Handle, 2000);
            }
            *active = 0;
        }
    }
    // Retain normal Windows termination of the visible launcher.
    0
}

struct SuspendedParent {
    process: OwnedHandle,
    id: u32,
    image: std::fs::File,
    delegated: bool,
}

impl SuspendedParent {
    fn create(path: &Path) -> Result<Self, String> {
        let directory = path
            .parent()
            .ok_or_else(|| "parent image needs a directory".to_string())?;
        // Lock before CreateProcess so a control event cannot terminate us in
        // the interval between process creation and publishing its handle.
        let mut active = ACTIVE_PARENT
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if *active != 0 {
            return Err("an owned suspended parent is already active".into());
        }
        if EXIT_REQUESTED.load(Ordering::SeqCst) {
            return Err("launch cancelled before parent creation".into());
        }
        let image = crate::acquisition::pin_image(path)?;
        let info = create_process(path, directory, &[], CREATE_SUSPENDED, &startup_info())?;
        *active = info.process.0 as usize;
        #[cfg(test)]
        LAST_CREATED_PARENT.store(info.id, std::sync::atomic::Ordering::Relaxed);
        Ok(Self {
            process: info.process,
            id: info.id,
            image,
            delegated: false,
        })
    }
}

impl Drop for SuspendedParent {
    fn drop(&mut self) {
        if self.delegated {
            return;
        }
        let mut active = ACTIVE_PARENT
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        // SAFETY: this guard owns this exact process handle; it never targets
        // an existing process by name or PID, and never resumes its thread.
        let result = unsafe {
            TerminateProcess(self.process.0, 1);
            WaitForSingleObject(self.process.0, 3000)
        };
        if *active == self.process.0 as usize {
            *active = 0;
        }
        if result != 0 {
            crate::logging::error(
                format!(
                    "无法确认自有挂起父进程 {} 已结束，等待结果：{result:#x}。",
                    self.id
                ),
                format!(
                    "Cleanup could not confirm termination of owned suspended parent {} (wait result {result:#x}).",
                    self.id
                ),
            );
        }
    }
}

struct AttributeList {
    storage: Vec<usize>,
    // UpdateProcThreadAttribute retains a pointer to the value. Box keeps its
    // address stable even when this Rust owner moves.
    values: Box<[Handle]>,
}

impl AttributeList {
    fn for_parent(parent: Handle) -> Result<Self, String> {
        Self::new(PROC_THREAD_ATTRIBUTE_PARENT_PROCESS, vec![parent])
    }

    fn new(attribute: usize, values: Vec<Handle>) -> Result<Self, String> {
        let mut size = 0;
        // SAFETY: documented size-query call; failure is expected at this stage.
        unsafe { InitializeProcThreadAttributeList(std::ptr::null_mut(), 1, 0, &mut size) };
        if size == 0 {
            return Err(format!(
                "failed to size process attribute list: {}",
                io::Error::last_os_error()
            ));
        }
        // Vec<usize> supplies pointer alignment, unlike Vec<u8>.
        let mut storage = vec![0usize; size.div_ceil(size_of::<usize>())];
        // SAFETY: storage is aligned and at least the reported byte size.
        if unsafe {
            InitializeProcThreadAttributeList(storage.as_mut_ptr().cast(), 1, 0, &mut size)
        } == 0
        {
            return Err(format!(
                "failed to initialize process attribute list: {}",
                io::Error::last_os_error()
            ));
        }
        let mut list = Self {
            storage,
            values: values.into_boxed_slice(),
        };
        // SAFETY: initialized list, a pointer-sized stable handle value, and
        // reserved arguments are zero/null. Both allocations live until Delete.
        if unsafe {
            UpdateProcThreadAttribute(
                list.as_mut_ptr(),
                0,
                attribute,
                list.values.as_mut_ptr().cast(),
                std::mem::size_of_val(&*list.values),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        } == 0
        {
            return Err(format!(
                "failed to set official parent process attribute: {}",
                io::Error::last_os_error()
            ));
        }
        Ok(list)
    }

    fn as_mut_ptr(&mut self) -> *mut c_void {
        self.storage.as_mut_ptr().cast()
    }
}

impl Drop for AttributeList {
    fn drop(&mut self) {
        // SAFETY: this list was successfully initialized and is deleted once,
        // before its backing storage and stable attribute value are released.
        unsafe { DeleteProcThreadAttributeList(self.as_mut_ptr()) };
    }
}

impl Child {
    pub fn try_wait(&self) -> Result<Option<u32>, String> {
        match unsafe { WaitForSingleObject(self.process.0, 0) } {
            258 => Ok(None),
            0 => self.wait().map(Some),
            _ => Err(format!(
                "cannot poll game status: {}",
                io::Error::last_os_error()
            )),
        }
    }

    pub fn cancelled(&self) -> bool {
        CANCELLED.load(Ordering::SeqCst)
    }

    pub fn id(&self) -> u32 {
        self.id
    }

    pub fn parent_id(&self) -> Option<u32> {
        self.parent.as_ref().map(|parent| parent.id)
    }

    pub fn wait(&self) -> Result<u32, String> {
        // SAFETY: the process handle is still owned by self throughout the call.
        let result = unsafe { WaitForSingleObject(self.process.0, u32::MAX) };
        if result != 0 {
            return Err(format!(
                "failed to wait for game: {} (wait result {result:#x})",
                io::Error::last_os_error()
            ));
        }
        let mut code = 0;
        // SAFETY: valid live process handle and writable u32 output.
        if unsafe { GetExitCodeProcess(self.process.0, &mut code) } == 0 {
            return Err(format!(
                "failed to obtain game exit code: {}",
                io::Error::last_os_error()
            ));
        }
        if let Some(guardian) = &self.guardian
            && unsafe { WaitForSingleObject(guardian.0, 5000) } != 0
        {
            return Err("game exited but guardian cleanup was not confirmed".into());
        }
        Ok(code)
    }
}

pub(super) fn stop_game() {
    CANCELLED.store(true, Ordering::SeqCst);
    let game = ACTIVE_GAME.lock().unwrap_or_else(|p| p.into_inner());
    if *game != 0 {
        // Only the explicit Ctrl+Z action terminates this owned game handle.
        unsafe {
            TerminateProcess(*game as Handle, 1223);
        }
    }
}

fn wide_nul(value: &OsStr) -> Result<Vec<u16>, String> {
    let mut wide: Vec<u16> = value.encode_wide().collect();
    if wide.contains(&0) {
        return Err("Windows paths and arguments must not contain NUL".into());
    }
    wide.push(0);
    Ok(wide)
}

// Windows CRT/CommandLineToArgvW-compatible argument quoting. Encoding units,
// including unpaired UTF-16 surrogates, remain unchanged.
fn quote_wide(value: &OsStr, output: &mut Vec<u16>, force: bool) -> Result<(), String> {
    let needs_quotes = force
        || value.is_empty()
        || value
            .encode_wide()
            .any(|unit| matches!(unit, 0x20 | 0x09 | 0x22));
    if !needs_quotes {
        for unit in value.encode_wide() {
            if unit == 0 {
                return Err("Windows arguments must not contain NUL".into());
            }
            output.push(unit);
        }
        return Ok(());
    }
    output.push(b'"' as u16);
    let mut slashes = 0;
    for unit in value.encode_wide() {
        if unit == 0 {
            return Err("Windows arguments must not contain NUL".into());
        }
        if unit == b'\\' as u16 {
            slashes += 1;
            continue;
        }
        let count = if unit == b'"' as u16 {
            slashes * 2 + 1
        } else {
            slashes
        };
        output.extend(std::iter::repeat_n(b'\\' as u16, count));
        output.push(unit);
        slashes = 0;
    }
    output.extend(std::iter::repeat_n(b'\\' as u16, slashes * 2));
    output.push(b'"' as u16);
    Ok(())
}

fn command_line(game: &OsStr, args: &[OsString]) -> Result<Vec<u16>, String> {
    let mut line = Vec::new();
    quote_wide(game, &mut line, true)?;
    for arg in args {
        line.push(b' ' as u16);
        quote_wide(arg, &mut line, false)?;
    }
    line.push(0);
    if line.len() > 32767 {
        return Err("command line exceeds the Windows 32767 UTF-16 unit limit".into());
    }
    Ok(line)
}

fn startup_info() -> StartupInfoW {
    // SAFETY: all-zero values are valid for this Win32 POD structure.
    let mut startup: StartupInfoW = unsafe { std::mem::zeroed() };
    startup.cb = size_of::<StartupInfoW>() as u32;
    startup
}

struct CreatedProcess {
    process: OwnedHandle,
    id: u32,
}

fn create_process(
    game: &Path,
    directory: &Path,
    args: &[OsString],
    flags: u32,
    startup: &StartupInfoW,
) -> Result<CreatedProcess, String> {
    create_process_impl(game, directory, args, flags, startup, false)
}

fn create_process_impl(
    game: &Path,
    directory: &Path,
    args: &[OsString],
    flags: u32,
    startup: &StartupInfoW,
    inherit: bool,
) -> Result<CreatedProcess, String> {
    let application = wide_nul(game.as_os_str())?;
    let directory = wide_nul(directory.as_os_str())?;
    let mut line = command_line(game.as_os_str(), args)?;
    // SAFETY: all-zero values are valid for this output structure.
    let mut info: ProcessInformation = unsafe { std::mem::zeroed() };
    // SAFETY: all strings are NUL-terminated, line is mutable as required by
    // CreateProcessW, and all backing allocations and structs live until return.
    // Null environment inherits the environment. Games inherit no handles;
    // the guardian inherits only its explicit handle list. With the extended flag,
    // security attributes are null and startup is the
    // leading field of a live StartupInfoExW and includes its full byte size.
    let created = unsafe {
        CreateProcessW(
            application.as_ptr(),
            line.as_mut_ptr(),
            null(),
            null(),
            i32::from(inherit),
            flags,
            null(),
            directory.as_ptr(),
            startup,
            &mut info,
        )
    };
    if created == 0 {
        let error = io::Error::last_os_error();
        if error.raw_os_error() == Some(740) {
            return Err("Windows error 740 (ERROR_ELEVATION_REQUIRED): run the launcher with administrator permission using its embedded manifest.".into());
        }
        return Err(format!("failed to create process {game:?}: {error}"));
    }
    let thread = OwnedHandle(info.thread);
    let child = CreatedProcess {
        process: OwnedHandle(info.process),
        id: info.process_id,
    };
    drop(thread);
    Ok(child)
}

pub fn launch(
    game: &Path,
    directory: &Path,
    args: &[OsString],
    parent_path: Option<&Path>,
) -> Result<Child, String> {
    // Both normal and direct waiting modes handle Ctrl+C. Register before
    // creating either process; guard removal follows game/parent cleanup.
    let control = ConsoleControl::install()?;
    if let Some(parent_path) = parent_path {
        let mut parent = SuspendedParent::create(parent_path)?;
        crate::logging::info(
            format!(
                "已从 {} 创建自有挂起父进程，PID：{}。",
                parent_path.display(),
                parent.id
            ),
            format!(
                "Created owned suspended parent PID {} from {}.",
                parent.id,
                parent_path.display()
            ),
        );
        let mut attributes = AttributeList::for_parent(parent.process.0)?;
        let mut extended = StartupInfoExW {
            startup: startup_info(),
            attribute_list: attributes.as_mut_ptr(),
        };
        extended.startup.cb = size_of::<StartupInfoExW>() as u32;
        let child = create_owned_game(
            game,
            directory,
            args,
            EXTENDED_STARTUPINFO_PRESENT,
            &extended.startup,
        )?;
        let guardian = match spawn_guardian(&parent, child.process.0) {
            Ok(guardian) => guardian,
            Err(error) => {
                // Failed launch setup: stop only the game just created here.
                stop_game();
                unsafe {
                    WaitForSingleObject(child.process.0, 3000);
                }
                *ACTIVE_GAME.lock().unwrap_or_else(|p| p.into_inner()) = 0;
                return Err(error);
            }
        };
        {
            let mut active = ACTIVE_PARENT.lock().unwrap_or_else(|p| p.into_inner());
            parent.delegated = true;
            *active = 0;
        }
        Ok(Child {
            process: child.process,
            id: child.id,
            parent: Some(parent),
            guardian: Some(guardian),
            _control: Some(control),
        })
    } else {
        let child = create_owned_game(game, directory, args, 0, &startup_info())?;
        Ok(Child {
            process: child.process,
            id: child.id,
            parent: None,
            guardian: None,
            _control: Some(control),
        })
    }
}

fn create_owned_game(
    game: &Path,
    directory: &Path,
    args: &[OsString],
    flags: u32,
    startup: &StartupInfoW,
) -> Result<CreatedProcess, String> {
    // Publish the handle before a control callback can run cleanup. Cancellation
    // during CreateProcess is handled immediately after this lock is released.
    let mut active = ACTIVE_GAME.lock().unwrap_or_else(|p| p.into_inner());
    if *active != 0 {
        return Err("an owned game is already active".into());
    }
    if EXIT_REQUESTED.load(Ordering::SeqCst) {
        return Err("launch cancelled before game creation".into());
    }
    let child = create_process(game, directory, args, flags, startup)?;
    *active = child.process.0 as usize;
    Ok(child)
}

fn inheritable_duplicate(handle: Handle) -> Result<OwnedHandle, String> {
    let mut duplicate = std::ptr::null_mut();
    // SAFETY: source is owned by the launch; DuplicateHandle creates a new owner.
    if unsafe {
        DuplicateHandle(
            GetCurrentProcess(),
            handle,
            GetCurrentProcess(),
            &mut duplicate,
            0,
            1,
            2,
        )
    } == 0
    {
        return Err(format!(
            "cannot duplicate guardian handle: {}",
            io::Error::last_os_error()
        ));
    }
    Ok(OwnedHandle(duplicate))
}

pub(crate) struct UpdateWorker {
    process: OwnedHandle,
}
impl UpdateWorker {
    pub(crate) fn is_running(&self) -> bool {
        unsafe { WaitForSingleObject(self.process.0, 0) == 258 }
    }
}

pub(crate) struct UpdateOwner {
    process: OwnedHandle,
    ready: OwnedHandle,
}
impl UpdateOwner {
    pub(crate) fn inherited(args: &[OsString]) -> Result<Self, String> {
        if args.len() != 2 {
            return Err("invalid update-worker handle arguments".into());
        }
        let mut handles = Vec::new();
        for arg in args {
            let value = arg
                .to_str()
                .and_then(|text| text.parse::<usize>().ok())
                .filter(|value| *value != 0 && *value != usize::MAX)
                .ok_or("invalid update-worker handle")?;
            let mut flags = 0;
            if unsafe { GetHandleInformation(value as Handle, &mut flags) } == 0 || flags & 1 == 0 {
                return Err("update worker requires inherited handles".into());
            }
            handles.push(value as Handle);
        }
        if handles[0] == handles[1]
            || unsafe { GetProcessId(handles[0]) } == 0
            || unsafe { GetProcessId(handles[0]) } == std::process::id()
        {
            return Err("invalid launcher owner identity".into());
        }
        Ok(Self {
            process: OwnedHandle(handles[0]),
            ready: OwnedHandle(handles[1]),
        })
    }
    pub(crate) fn acknowledge(&self) -> Result<(), String> {
        if unsafe { SetEvent(self.ready.0) } == 0 {
            return Err("update readiness acknowledgement failed".into());
        }
        Ok(())
    }
    pub(crate) fn exited(&self, timeout: u32) -> Result<bool, String> {
        match unsafe { WaitForSingleObject(self.process.0, timeout) } {
            0 => Ok(true),
            258 => Ok(false),
            _ => Err("cannot wait for launcher owner exit".into()),
        }
    }
}

pub(crate) fn spawn_update_worker(executable: &Path) -> Result<UpdateWorker, String> {
    spawn_update_worker_for_owner(executable, unsafe { GetCurrentProcess() })
}
fn spawn_update_worker_for_owner(executable: &Path, owner: Handle) -> Result<UpdateWorker, String> {
    let ready = unsafe { CreateEventW(null(), 1, 0, null()) };
    if ready.is_null() {
        return Err("cannot create update readiness event".into());
    }
    let ready = OwnedHandle(ready);
    let mut owner_duplicate = null::<c_void>() as Handle;
    // The helper can query/wait for its exact owner, but cannot terminate it.
    if unsafe {
        DuplicateHandle(
            GetCurrentProcess(),
            owner,
            GetCurrentProcess(),
            &mut owner_duplicate,
            0x0010_1000,
            1,
            0,
        )
    } == 0
    {
        return Err(format!(
            "cannot inherit launcher wait handle: {}",
            io::Error::last_os_error()
        ));
    }
    let inherited = [
        OwnedHandle(owner_duplicate),
        inheritable_duplicate(ready.0)?,
    ];
    let handles: Vec<_> = inherited.iter().map(|handle| handle.0).collect();
    let args = vec![
        OsString::from(crate::update_worker::WORKER_FLAG),
        (handles[0] as usize).to_string().into(),
        (handles[1] as usize).to_string().into(),
    ];
    let mut attributes = AttributeList::new(PROC_THREAD_ATTRIBUTE_HANDLE_LIST, handles)?;
    let mut startup = StartupInfoExW {
        startup: startup_info(),
        attribute_list: attributes.as_mut_ptr(),
    };
    startup.startup.cb = size_of::<StartupInfoExW>() as u32;
    let worker = create_process_impl(
        executable,
        executable.parent().ok_or("worker has no directory")?,
        &args,
        0x0800_0000 | EXTENDED_STARTUPINFO_PRESENT,
        &startup.startup,
        true,
    )?;
    if unsafe { WaitForMultipleObjects(2, [ready.0, worker.process.0].as_ptr(), 0, 10_000) } != 0 {
        unsafe {
            TerminateProcess(worker.process.0, 1);
            WaitForSingleObject(worker.process.0, 3000);
        }
        return Err("background update worker did not acknowledge readiness".into());
    }
    Ok(UpdateWorker {
        process: worker.process,
    })
}

fn spawn_guardian(parent: &SuspendedParent, game: Handle) -> Result<OwnedHandle, String> {
    #[cfg(not(test))]
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    #[cfg(test)]
    let executable =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/debug/process-probe.exe");
    let directory = executable
        .parent()
        .ok_or("guardian executable has no directory")?;
    let ready = unsafe { CreateEventW(null(), 1, 0, null()) };
    if ready.is_null() {
        return Err(format!(
            "cannot create guardian ready event: {}",
            io::Error::last_os_error()
        ));
    }
    let ready = OwnedHandle(ready);
    // The guardian inherits only these four exact handles, never unrelated handles
    // belonging to a terminal, credentials, or other launcher processes.
    let inherited = [
        inheritable_duplicate(parent.process.0)?,
        inheritable_duplicate(game)?,
        inheritable_duplicate(parent.image.as_raw_handle())?,
        inheritable_duplicate(ready.0)?,
    ];
    let handles: Vec<_> = inherited.iter().map(|handle| handle.0).collect();
    let mut args = vec![OsString::from(GUARDIAN_FLAG)];
    args.extend(
        handles
            .iter()
            .map(|handle| OsString::from((*handle as usize).to_string())),
    );
    let mut attributes = AttributeList::new(PROC_THREAD_ATTRIBUTE_HANDLE_LIST, handles)?;
    let mut startup = StartupInfoExW {
        startup: startup_info(),
        attribute_list: attributes.as_mut_ptr(),
    };
    startup.startup.cb = size_of::<StartupInfoExW>() as u32;
    let guardian = create_process_impl(
        &executable,
        directory,
        &args,
        0x0800_0000 | EXTENDED_STARTUPINFO_PRESENT,
        &startup.startup,
        true,
    )?;
    let waits = [ready.0, guardian.process.0];
    // A positive acknowledgement proves the helper validated and owns the handles.
    if unsafe { WaitForMultipleObjects(2, waits.as_ptr(), 0, 10_000) } != 0 {
        unsafe {
            TerminateProcess(guardian.process.0, 1);
            WaitForSingleObject(guardian.process.0, 3000);
        }
        return Err("background parent guardian failed to become ready".into());
    }
    crate::logging::info(
        format!(
            "后台父进程守护已就绪，PID：{}；关闭启动器后游戏将继续运行。",
            guardian.id
        ),
        format!(
            "Background parent guardian ready, PID: {}; the game will keep running after launcher exit.",
            guardian.id
        ),
    );
    Ok(guardian.process)
}

pub(super) fn guardian_entry(args: &[OsString]) -> Result<(), String> {
    if args.len() != 4 {
        return Err("invalid internal guardian arguments".into());
    }
    let handles: Vec<Handle> = args
        .iter()
        .map(|value| {
            value
                .to_str()
                .and_then(|value| value.parse::<usize>().ok())
                .filter(|value| *value != 0 && *value != usize::MAX)
                .map(|value| value as Handle)
                .ok_or("invalid inherited guardian handle")
        })
        .collect::<Result<_, _>>()?;
    let mut unique = std::collections::HashSet::new();
    for handle in &handles {
        let mut flags = 0;
        if !unique.insert(*handle as usize)
            || unsafe { GetHandleInformation(*handle, &mut flags) } == 0
            || flags & 1 == 0
        {
            return Err("guardian requires distinct inherited handles".into());
        }
    }
    let parent_id = unsafe { GetProcessId(handles[0]) };
    let game_id = unsafe { GetProcessId(handles[1]) };
    if parent_id == 0
        || game_id == 0
        || parent_id == game_id
        || parent_id == std::process::id()
        || game_id == std::process::id()
        || unsafe { GetFileType(handles[2]) } != 1
    {
        return Err("invalid guardian process or image identities".into());
    }
    // These are inherited references to the exact original process/file objects;
    // no reopening by PID and no PID-reuse-sensitive cleanup takes place.
    let parent = OwnedHandle(handles[0]);
    let game = OwnedHandle(handles[1]);
    let _image_pin = OwnedHandle(handles[2]);
    let ready = OwnedHandle(handles[3]);
    if unsafe { SetEvent(ready.0) } == 0 {
        return Err("cannot acknowledge guardian readiness".into());
    }
    if unsafe { WaitForSingleObject(game.0, u32::MAX) } != 0 {
        return Err("guardian game wait failed".into());
    }
    unsafe {
        TerminateProcess(parent.0, 1);
    }
    if unsafe { WaitForSingleObject(parent.0, 5000) } != 0 {
        return Err("guardian parent cleanup failed".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::windows::ffi::OsStringExt;

    #[test]
    #[ignore = "network and workspace-only updater-owner exit probe; build process-probe first"]
    fn workspace_update_worker_applies_only_after_launcher_exit() {
        use sha2::{Digest, Sha256};
        struct Fixture(CreatedProcess);
        impl Drop for Fixture {
            fn drop(&mut self) {
                unsafe {
                    TerminateProcess(self.0.process.0, 1);
                    WaitForSingleObject(self.0.process.0, 3000);
                }
            }
        }
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = root.join(format!("artifacts/update-worker-probe-{stamp}"));
        std::fs::create_dir_all(&directory).unwrap();
        let probe = root.join("target/debug/process-probe.exe");
        let target = directory.join("endfield-direct-launch.exe");
        std::fs::copy(&probe, &target).unwrap();
        let old_bytes = std::fs::read(&target).unwrap();
        let owner = Fixture(
            create_process(
                &target,
                &directory,
                &["--stay-alive".into()],
                0,
                &startup_info(),
            )
            .unwrap(),
        );
        let game_like = Fixture(
            create_process(
                &target,
                &directory,
                &["--stay-alive".into()],
                0,
                &startup_info(),
            )
            .unwrap(),
        );
        let helper =
            crate::update_worker::create_layout(&target, &probe, &[], "0.0.0", "local", "local")
                .unwrap();
        let worker = spawn_update_worker_for_owner(&helper, owner.0.process.0).unwrap();
        let state_path = helper.with_file_name("state.json");
        let started = std::time::Instant::now();
        loop {
            let state = std::fs::read(&state_path)
                .ok()
                .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok());
            if state
                .as_ref()
                .is_some_and(|value| value["phase"] == "staged")
            {
                break;
            }
            if started.elapsed().as_secs() > 55
                || state
                    .as_ref()
                    .is_some_and(|value| value["phase"] == "error")
            {
                unsafe {
                    TerminateProcess(worker.process.0, 1);
                    WaitForSingleObject(worker.process.0, 3000);
                }
                panic!(
                    "worker did not stage update: {}",
                    std::fs::read_to_string(helper.with_file_name("events.jsonl"))
                        .unwrap_or_default()
                );
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        assert_eq!(
            std::fs::read(&target).unwrap(),
            old_bytes,
            "live EXE must remain unchanged until the owner closes"
        );
        assert_eq!(unsafe { WaitForSingleObject(owner.0.process.0, 0) }, 258);
        assert_eq!(
            unsafe { WaitForSingleObject(game_like.0.process.0, 0) },
            258
        );
        let receipt_path = std::fs::read_dir(directory.join("launcher-update"))
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.path().join("receipt.json"))
            .find(|path| path.is_file())
            .unwrap();
        let receipt: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&receipt_path).unwrap()).unwrap();
        let backup = std::path::PathBuf::from(receipt["backup"].as_str().unwrap());
        assert!(!backup.exists(), "backup rename is deferred too");
        drop(owner);
        assert_eq!(
            unsafe { WaitForSingleObject(worker.process.0, 15_000) },
            0,
            "worker must finish after its exact owner exits"
        );
        let state: serde_json::Value =
            serde_json::from_slice(&std::fs::read(state_path).unwrap()).unwrap();
        assert_eq!(state["phase"], "applied");
        assert_eq!(std::fs::read(&backup).unwrap(), old_bytes);
        let new_hash = format!("{:x}", Sha256::digest(std::fs::read(&target).unwrap()));
        assert_eq!(receipt["new_sha256"], new_hash);
        assert_eq!(
            unsafe { WaitForSingleObject(game_like.0.process.0, 0) },
            258,
            "another loaded old image must keep running"
        );
        let journal = std::fs::read_to_string(helper.with_file_name("events.jsonl")).unwrap();
        for line in journal.lines() {
            let _: crate::logging::JournalRecord = serde_json::from_str(line).unwrap();
        }
        std::fs::write(root.join("analysis/update-after-exit-probe.json"), serde_json::to_vec_pretty(&serde_json::json!({
            "tag":receipt["tag"],"download_route":receipt["download_route"],"live_exe_unchanged_until_close":true,
            "owner_exit_applied_update":true,"other_loaded_process_kept_running":true,"backup_matches_original":true,
            "bilingual_journal_valid":true,"downloaded_exe_executed":false,"target":target,"sha256":new_hash
        })).unwrap()).unwrap();
    }

    #[test]
    #[ignore = "isolated console host; run via workspace_console_ctrl_c_and_menu_probe"]
    fn isolated_console_host() {
        let original_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let _ = std::fs::write(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("analysis/console-probe-panic.txt"),
                info.to_string(),
            );
            original_hook(info);
        }));
        let mut console_processes = [0; 8];
        if unsafe {
            windows_sys::Win32::System::Console::GetConsoleProcessList(
                console_processes.as_mut_ptr(),
                8,
            )
        } != 1
            || console_processes[0] != std::process::id()
        {
            // Never dispatch a broadcast event into the user's shared console.
            return;
        }
        struct Fixture(CreatedProcess);
        impl Drop for Fixture {
            fn drop(&mut self) {
                unsafe {
                    TerminateProcess(self.0.process.0, 1);
                    WaitForSingleObject(self.0.process.0, 3000);
                }
            }
        }
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let probe = root.join("target/debug/process-probe.exe");
        let unrelated = Fixture(
            create_process(&probe, &root, &["--stay-alive".into()], 0, &startup_info()).unwrap(),
        );
        let child = launch(&probe, &root, &["--stay-alive".into()], Some(&probe)).unwrap();
        let parent_pid = child.parent_id().unwrap();
        // A console close callback must leave both the game and parent alive.
        assert_eq!(console_control_handler(2), 0);
        assert_eq!(unsafe { WaitForSingleObject(child.process.0, 0) }, 258);
        assert_eq!(
            unsafe { WaitForSingleObject(child.parent.as_ref().unwrap().process.0, 0) },
            258
        );
        assert!(!child.cancelled());
        stop_game();
        assert_eq!(unsafe { WaitForSingleObject(child.process.0, 5000) }, 0);
        assert_eq!(child.wait().unwrap(), 1223);
        assert!(child.cancelled());
        // Synchronize with the whole callback before inspecting its parent cleanup.
        {
            let _game_guard = ACTIVE_GAME.lock().unwrap();
        }
        assert_eq!(
            unsafe { WaitForSingleObject(child.parent.as_ref().unwrap().process.0, 0) },
            0
        );
        drop(child);
        assert_eq!(*ACTIVE_PARENT.lock().unwrap(), 0);
        assert_eq!(*ACTIVE_GAME.lock().unwrap(), 0);
        assert_eq!(
            unsafe { WaitForSingleObject(unrelated.0.process.0, 0) },
            258
        );

        // A second launch must reset cancellation and handle direct waiting mode.
        let direct = crate::Session {
            child: launch(&probe, &root, &["--stay-alive".into()], None).unwrap(),
            _managed_parent: None,
        };
        assert!(!direct.child.cancelled());
        crate::menu::probe_stop_key(&direct);
        assert_eq!(direct.child.wait().unwrap(), 1223);
        assert!(direct.child.cancelled());
        drop(direct);
        crate::menu::probe_console_keys();
        let menu_game = crate::Session {
            child: launch(&probe, &root, &["--stay-alive".into()], Some(&probe)).unwrap(),
            _managed_parent: None,
        };
        crate::menu::probe_running_menu(menu_game);
        assert_eq!(*ACTIVE_PARENT.lock().unwrap(), 0);
        assert_eq!(*ACTIVE_GAME.lock().unwrap(), 0);
        assert_eq!(
            unsafe { WaitForSingleObject(unrelated.0.process.0, 0) },
            258
        );
        let exit_game = crate::Session {
            child: launch(&probe, &root, &["--stay-alive".into()], Some(&probe)).unwrap(),
            _managed_parent: None,
        };
        let retained_game = Fixture(CreatedProcess {
            process: inheritable_duplicate(exit_game.child.process.0).unwrap(),
            id: exit_game.child.id(),
        });
        let retained_parent =
            inheritable_duplicate(exit_game.child.parent.as_ref().unwrap().process.0).unwrap();
        let retained_guardian =
            inheritable_duplicate(exit_game.child.guardian.as_ref().unwrap().0).unwrap();
        crate::menu::probe_exit_menu(exit_game);
        assert_eq!(
            unsafe { WaitForSingleObject(retained_game.0.process.0, 0) },
            258
        );
        assert_eq!(unsafe { WaitForSingleObject(retained_parent.0, 0) }, 258);
        assert_eq!(unsafe { WaitForSingleObject(retained_guardian.0, 0) }, 258);
        drop(retained_game);
        assert_eq!(unsafe { WaitForSingleObject(retained_guardian.0, 5000) }, 0);
        assert_eq!(unsafe { WaitForSingleObject(retained_parent.0, 0) }, 0);
        // Drive actual start/restart commands using only workspace copies of the fixture.
        let restart_root = root.join("artifacts/menu-restart-probe");
        std::fs::create_dir_all(&restart_root).unwrap();
        let fixture_game = restart_root.join("Endfield.exe");
        let fixture_parent = restart_root.join("Games.exe");
        std::fs::copy(&probe, &fixture_game).unwrap();
        std::fs::copy(&probe, &fixture_parent).unwrap();
        let restart_options = crate::Options {
            game: fixture_game,
            parent: Some(fixture_parent),
            extra: vec!["--stay-alive".into()],
            ..Default::default()
        };
        let started =
            crate::menu::probe_launch_commands(restart_options.clone(), None, &["start", "exit"]);
        let first_pid = started.child.id();
        let first_game = inheritable_duplicate(started.child.process.0).unwrap();
        let first_parent =
            inheritable_duplicate(started.child.parent.as_ref().unwrap().process.0).unwrap();
        let first_guardian =
            inheritable_duplicate(started.child.guardian.as_ref().unwrap().0).unwrap();
        let restarted = crate::menu::probe_launch_commands(
            restart_options.clone(),
            Some(started),
            &["start", "restart", "exit"],
        );
        assert_ne!(restarted.child.id(), first_pid);
        for handle in [&first_game, &first_parent, &first_guardian] {
            assert_eq!(unsafe { WaitForSingleObject(handle.0, 0) }, 0);
        }
        assert_eq!(
            unsafe { WaitForSingleObject(restarted.child.process.0, 0) },
            258
        );
        assert!(!restarted.child.cancelled());
        restarted.stop().unwrap();
        drop(restarted);
        let from_stopped =
            crate::menu::probe_launch_commands(restart_options, None, &["restart", "exit"]);
        assert_eq!(
            unsafe { WaitForSingleObject(from_stopped.child.process.0, 0) },
            258
        );
        assert_eq!(
            unsafe { WaitForSingleObject(unrelated.0.process.0, 0) },
            258
        );
        from_stopped.stop().unwrap();
        drop(from_stopped);
        assert_eq!(*ACTIVE_GAME.lock().unwrap(), 0);
        assert_eq!(*ACTIVE_PARENT.lock().unwrap(), 0);
        std::fs::write(root.join("analysis/restart-command-probe.txt"),
            "Workspace fixtures only; no official process executed.\nstart-command-created-game=true\nrestart-created-new-game=true\nprevious-game-parent-guardian-cleaned=true\nrestart-without-running-game-started=true\nexit-after-restart-keeps-game-running=true\nunrelated-fixture-untouched=true\nall-owned-fixtures-cleaned=true\n"
        ).unwrap();
        std::fs::write(root.join("analysis/menu-control-probe.txt"), format!(
            "Workspace fixtures only; no official game or launcher executed.\nconsole-close-callback-preserves-game-and-parent=true\nCtrl+Z-owned-game-terminated=true\nowned-parent-terminated-after-game-exit=true\nparent-pid={parent_pid}\nunrelated-fixture-untouched=true\nsecond-launch-cancellation-reset=true\nCtrl+Z consumed through Windows console input and game stopped=true\nre/Tab/Enter consumed through Windows console input; no action before Enter=true\nlive-candidates-rendered-with-cursor-before-hint=true\nselected-candidate-rendered-and-cleared-before-output=true\ncn-command-replayed-history-while-game-running=true\nstartup-console-log-preserved-after-banner=true\nmouse-input-disabled-and-host-quick-edit-preserved=true\nnative-mouse-wheel-scroll-tested=true\nconsole-input-mode-restored=true\nCommand console remained responsive while game fixture ran; cn -> Ctrl+Z -> exit completed=true\nmenu-exit-preserves-game-parent-and-guardian=true\nguardian-exits-and-cleans-parent-after-game-exit=true\n"
        )).unwrap();
    }

    #[test]
    #[ignore = "build process-probe first; uses an isolated hidden console and workspace fixtures"]
    fn workspace_console_ctrl_c_and_menu_probe() {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let harness = std::env::current_exe().unwrap();
        let report = root.join("analysis/menu-control-probe.txt");
        if report.exists() {
            std::fs::remove_file(&report).unwrap();
        }
        let args: Vec<OsString> = [
            "--ignored",
            "--exact",
            "windows::tests::isolated_console_host",
            "--nocapture",
            "--test-threads=1",
        ]
        .into_iter()
        .map(OsString::from)
        .collect();
        let mut startup = startup_info();
        startup.flags = 1; // STARTF_USESHOWWINDOW
        startup.show_window = 0; // SW_HIDE for the automated host.
        let host = create_process(&harness, &root, &args, 0x10, &startup).unwrap();
        let result = unsafe { WaitForSingleObject(host.process.0, 25_000) };
        if result != 0 {
            unsafe {
                TerminateProcess(host.process.0, 1);
                WaitForSingleObject(host.process.0, 3000);
            }
            panic!("isolated console probe timed out or wait failed: {result}");
        }
        let mut status = 1;
        assert_ne!(
            unsafe { GetExitCodeProcess(host.process.0, &mut status) },
            0
        );
        assert_eq!(status, 0, "isolated probe failed");
        assert!(
            std::fs::read_to_string(root.join("analysis/menu-control-probe.txt"))
                .unwrap()
                .contains("menu-exit-preserves-game-parent-and-guardian=true")
        );
    }

    #[test]
    #[ignore = "private hidden console host; used by workspace_launcher_close_preserves_game"]
    fn launcher_close_target_host() {
        use windows_sys::Win32::System::Console::{GetConsoleProcessList, GetConsoleWindow};
        let mut processes = [0; 8];
        if unsafe { GetConsoleProcessList(processes.as_mut_ptr(), 8) } != 1
            || processes[0] != std::process::id()
        {
            return;
        }
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let directory = root
            .join("target")
            .join(format!("close-host-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let probe = root.join("target/debug/process-probe.exe");
        let parent_image = directory.join("Games.exe");
        std::fs::copy(&probe, &parent_image).unwrap();
        let child = launch(
            &probe,
            &directory,
            &["--stay-alive".into()],
            Some(&parent_image),
        )
        .unwrap();
        let data = serde_json::json!({
            "host": std::process::id(), "window": unsafe { GetConsoleWindow() } as usize,
            "game": child.id(), "parent": child.parent_id().unwrap(),
            "guardian": unsafe { GetProcessId(child.guardian.as_ref().unwrap().0) },
        });
        let path = root.join("analysis/launcher-close-host-ready.json");
        let pending = root.join("analysis/launcher-close-host-ready.tmp");
        std::fs::write(&pending, serde_json::to_vec(&data).unwrap()).unwrap();
        std::fs::rename(pending, path).unwrap();
        std::thread::sleep(std::time::Duration::from_secs(30));
        stop_game(); // Failsafe if the supervising test never closes this host.
        let _ = child.wait();
    }

    #[test]
    #[ignore = "closes only private hidden fixture consoles; build process-probe first"]
    fn workspace_launcher_close_preserves_game() {
        #[link(name = "user32")]
        unsafe extern "system" {
            fn PostMessageW(window: Handle, message: u32, wparam: usize, lparam: isize) -> i32;
        }
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn OpenProcess(access: u32, inherit: i32, process_id: u32) -> Handle;
        }
        struct Fixture(OwnedHandle);
        impl Drop for Fixture {
            fn drop(&mut self) {
                unsafe {
                    TerminateProcess(self.0.0, 1);
                    WaitForSingleObject(self.0.0, 3000);
                }
            }
        }
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let ready = root.join("analysis/launcher-close-host-ready.json");
        let mut observations = Vec::new();
        for mode in ["window-close", "force-kill-visible-host"] {
            if ready.exists() {
                std::fs::remove_file(&ready).unwrap();
            }
            let harness = std::env::current_exe().unwrap();
            let args: Vec<OsString> = [
                "--ignored",
                "--exact",
                "windows::tests::launcher_close_target_host",
                "--nocapture",
                "--test-threads=1",
            ]
            .into_iter()
            .map(OsString::from)
            .collect();
            let mut startup = startup_info();
            startup.flags = 1;
            startup.show_window = 0;
            let host = create_process(&harness, &root, &args, 0x10, &startup).unwrap();
            let host_pid = host.id;
            let host = Fixture(host.process);
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
            while !ready.exists() && std::time::Instant::now() < deadline {
                std::thread::sleep(std::time::Duration::from_millis(30));
            }
            let data: serde_json::Value = serde_json::from_slice(
                &std::fs::read(&ready).expect("fixture host failed to become ready"),
            )
            .unwrap();
            assert_eq!(data["host"].as_u64().unwrap(), host_pid as u64);
            let open = |key: &str| {
                let pid = data[key].as_u64().unwrap() as u32;
                let handle = unsafe { OpenProcess(0x0010_1001, 0, pid) };
                assert!(!handle.is_null(), "cannot inspect fixture {key} PID {pid}");
                OwnedHandle(handle)
            };
            let game = Fixture(open("game"));
            let parent = open("parent");
            let guardian = open("guardian");
            if mode == "window-close" {
                let window = data["window"].as_u64().unwrap() as Handle;
                assert!(!window.is_null());
                assert_ne!(unsafe { PostMessageW(window, 0x10, 0, 0) }, 0);
            } else {
                assert_ne!(unsafe { TerminateProcess(host.0.0, 1) }, 0);
            }
            assert_eq!(unsafe { WaitForSingleObject(host.0.0, 10_000) }, 0);
            assert_eq!(
                unsafe { WaitForSingleObject(game.0.0, 0) },
                258,
                "game stopped with visible launcher"
            );
            assert_eq!(
                unsafe { WaitForSingleObject(parent.0, 0) },
                258,
                "official parent stopped before game exit"
            );
            assert_eq!(unsafe { WaitForSingleObject(guardian.0, 0) }, 258);
            drop(game);
            assert_eq!(unsafe { WaitForSingleObject(guardian.0, 5000) }, 0);
            assert_eq!(unsafe { WaitForSingleObject(parent.0, 0) }, 0);
            let mut code = 1;
            assert_ne!(unsafe { GetExitCodeProcess(guardian.0, &mut code) }, 0);
            assert_eq!(code, 0);
            observations.push(serde_json::json!({"mode": mode, "launcher_exited": true, "game_kept_running": true, "parent_kept_running": true, "guardian_cleaned_parent_after_game_exit": true, "guardian_exited": true}));
        }
        std::fs::write(
            root.join("analysis/launcher-detach-probe.json"),
            serde_json::to_vec_pretty(&observations).unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn win32_startup_layout_matches_64_bit_abi() {
        if size_of::<usize>() == 8 {
            assert_eq!(size_of::<StartupInfoW>(), 104);
            assert_eq!(size_of::<StartupInfoExW>(), 112);
            assert_eq!(size_of::<ProcessInformation>(), 24);
        }
    }

    #[test]
    fn command_line_quotes_spaces_empty_args_quotes_and_trailing_slashes() {
        let args = [
            OsString::from(""),
            OsString::from("中文 空格"),
            OsString::from("a\\\"b"),
            OsString::from("a b\\"),
            OsString::from("a\\"),
            OsString::from("a\tb"),
        ];
        let line = command_line(OsStr::new(r"D:\space dir\Endfield.exe"), &args).unwrap();
        assert_eq!(
            String::from_utf16(&line[..line.len() - 1]).unwrap(),
            "\"D:\\space dir\\Endfield.exe\" \"\" \"中文 空格\" \"a\\\\\\\"b\" \"a b\\\\\" a\\ \"a\tb\""
        );
    }

    #[test]
    fn command_line_preserves_unpaired_surrogates() {
        let arg = OsString::from_wide(&[0xd800, 0x20, 0xdc00]);
        let line = command_line(OsStr::new("x"), &[arg]).unwrap();
        assert_eq!(line, [34, 120, 34, 32, 34, 0xd800, 32, 0xdc00, 34, 0]);
    }

    #[test]
    fn default_raw_command_line_matches_official_launcher_template() {
        let args = crate::game_args(&crate::Options::default());
        let line = command_line(OsStr::new(crate::DEFAULT_GAME), &args).unwrap();
        assert_eq!(
            String::from_utf16(&line[..line.len() - 1]).unwrap(),
            format!(
                "\"{}\" -launcher_lang=zh-cn -launcher_sub_channel=1",
                crate::DEFAULT_GAME
            )
        );
    }

    #[test]
    fn rejects_nul_and_oversize_before_any_process_creation() {
        assert!(command_line(OsStr::new("x"), &[OsString::from("a\0b")]).is_err());
        assert!(command_line(OsStr::new("x"), &[OsString::from("a".repeat(32767))]).is_err());
        assert!(wide_nul(OsStr::new("a\0b")).is_err());
    }

    #[test]
    #[ignore = "build process-probe first; executes workspace-only fixture processes"]
    fn workspace_probe_validates_parent_image_argv_and_cleanup() {
        use std::path::PathBuf;
        use std::sync::atomic::Ordering;

        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn OpenProcess(access: u32, inherit: i32, process_id: u32) -> Handle;
        }

        fn ended(pid: u32) -> bool {
            // SAFETY: query only; a missing process means it has already exited.
            let handle = unsafe { OpenProcess(0x0010_0000, 0, pid) };
            if handle.is_null() {
                return true;
            }
            let handle = OwnedHandle(handle);
            // SAFETY: live synchronization handle, nonblocking query.
            unsafe { WaitForSingleObject(handle.0, 0) == 0 }
        }

        struct FixtureGuard(CreatedProcess);
        impl Drop for FixtureGuard {
            fn drop(&mut self) {
                // SAFETY: exclusively owns this workspace fixture process.
                unsafe {
                    TerminateProcess(self.0.process.0, 1);
                    WaitForSingleObject(self.0.process.0, 3000);
                }
            }
        }

        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let probe = root.join("target").join("debug").join("process-probe.exe");
        assert!(
            probe.is_file(),
            "run cargo build --offline --bin process-probe first"
        );
        let directory = root
            .join("target")
            .join(format!("probe-fixtures-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let parent_image = directory.join("custom-parent.exe");
        std::fs::copy(&probe, &parent_image).unwrap();
        let _pinned_image = crate::acquisition::pin_image(&parent_image).unwrap();
        let marker = directory.join("parent-main-ran.marker");
        assert!(!marker.exists());
        let output = directory.join("child-report.txt");
        let expected_args: Vec<OsString> =
            ["中文 空格", "a\\\"b", "a b\\", "", "tab\tvalue", "plain"]
                .into_iter()
                .map(OsString::from)
                .collect();
        let mut args = vec![output.clone().into_os_string()];
        args.extend(expected_args.iter().cloned());

        // An additional workspace-owned suspended process is deliberately not
        // registered as ACTIVE_PARENT. Cleanup must leave it untouched.
        let unrelated = FixtureGuard(
            create_process(
                &parent_image,
                &directory,
                &[],
                CREATE_SUSPENDED,
                &startup_info(),
            )
            .unwrap(),
        );
        let child = launch(&probe, &directory, &args, Some(&parent_image)).unwrap();
        let parent_pid = child.parent_id().unwrap();
        let child_pid = child.id();
        // The short-lived probe may already have exited during guardian handoff;
        // the slow fixture in the console probe checks the live-parent lifetime.
        assert_eq!(child.wait().unwrap(), 0);
        let report = std::fs::read_to_string(&output).unwrap();
        assert!(report.contains(&format!("parent-pid={parent_pid}\n")));
        assert!(report.contains(&format!("parent-image={}\n", parent_image.display())));
        assert!(report.contains(&format!("args={expected_args:?}\n")));
        drop(child);
        assert!(ended(parent_pid));
        assert!(
            !marker.exists(),
            "suspended parent's main must never execute"
        );
        let mut results = format!(
            "Workspace fixtures only; no official executable was started.\nchild-pid={child_pid}\n{report}success-parent-cleaned=true\nparent-main-marker-absent=true\npinned-parent-image-compatible=true\n"
        );

        let missing = directory.join("does-not-exist.exe");
        let error = match launch(&missing, &directory, &[], Some(&parent_image)) {
            Ok(_) => panic!("nonexistent child unexpectedly started"),
            Err(error) => error,
        };
        let failed_parent_pid = LAST_CREATED_PARENT.load(Ordering::Relaxed);
        assert!(ended(failed_parent_pid));
        assert_eq!(*ACTIVE_PARENT.lock().unwrap(), 0);
        results.push_str(&format!(
            "failure-parent-pid={failed_parent_pid}\nfailure={error}\nfailure-parent-cleaned=true\n"
        ));

        let control = ConsoleControl::install().unwrap();
        let parent = SuspendedParent::create(&parent_image).unwrap();
        let cancelled_parent_pid = parent.id;
        assert_eq!(console_control_handler(0), 0);
        // SAFETY: the parent guard still owns its handle after handler cleanup.
        assert_eq!(unsafe { WaitForSingleObject(parent.process.0, 3000) }, 0);
        drop(parent);
        drop(control);
        assert!(ended(cancelled_parent_pid));
        assert_eq!(*ACTIVE_PARENT.lock().unwrap(), 0);
        // SAFETY: fixture guard still owns this deliberately unrelated process.
        assert_eq!(
            unsafe { WaitForSingleObject(unrelated.0.process.0, 0) },
            258
        );
        assert!(!marker.exists());
        results.push_str(&format!("handler-parent-pid={cancelled_parent_pid}\nhandler-parent-cleaned=true\nunrelated-fixture-untouched=true\nparent-main-marker-absent-after-all=true\nControl cleanup callback called directly; actual console event dispatch was not tested.\n"));
        drop(unrelated);
        std::fs::create_dir_all(root.join("analysis")).unwrap();
        std::fs::write(root.join("analysis/process-probe-result.txt"), &results).unwrap();
        println!("{results}");
    }
}
