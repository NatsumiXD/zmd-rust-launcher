use std::env;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

mod acquisition;
mod language;
mod logging;
#[cfg(windows)]
mod menu;
#[cfg(windows)]
mod native_acquire;
mod self_update;
#[cfg(windows)]
mod update_worker;
#[cfg(windows)]
mod windows;

const DEFAULT_GAME: &str = r"D:\yj\Hypergryph Launcher\games\Arknights Endfield\Endfield.exe";
const HELP: &str = "endfield-direct-launch [OPTIONS] [-- EXTRA_GAME_ARGS...]

Default: print startup logs, start the game, then open the command console.
Commands: start, restart, update, lud, preview, status, stop, language, cn, help, exit.
lud checks/benchmarks/downloads Rust launcher updates; applies after launcher exit.
Every successful game launch also requests this background update check.
restart stops this session's game, waits for cleanup, then checks updates and starts again.
language toggles the saved display language; language cn / language en selects it.
cn prints earlier session logs in Chinese without changing the display language.
Candidates appear as you type; Tab / Shift+Tab cycle completions.
Enter runs the typed command; Up/Down recalls command history.
With command-line options: check/download if needed, then start the game.

  --game PATH          Absolute path to Endfield.exe
  --cache-dir PATH     Managed cache (default: endfield-cache beside this EXE)
  --parent PATH        Explicit official image; skips managed updates
  --offline            Use a verified local/cached Games.exe without networking
  --no-local-parent    Skip local Games.exe discovery; use managed cache/download
  --direct             Direct launch without official parent (previous game test failed)
  --lang VALUE         Launcher language (default: zh-cn)
  --render MODE        default | dx11 | vk (vk adds no unverified flag)
  --sub-channel VALUE  launcher_sub_channel (default: 1; none omits it)
  --tracking-code VALUE Optional tracking_code
  --menu               Open the command console without automatically starting the game
  --launch             Check/download if needed, then launch without a menu
  --update-only        Check/download Games.exe without starting the game
  --dry-run            Preview only; no network, downloads, or process creation
  --wait               Wait for direct mode; official-parent mode always waits
  -h, --help           Show this help

Default launch creates an official parent with its main thread suspended,
then starts the game using the Windows parent-process creation attribute.
The official parent's main thread is never resumed. This tool waits for the
game and terminates only its own suspended parent afterward.
Ctrl+Z closes this tool's game and parent while keeping the menu open.
Closing the launcher leaves the game running; a hidden guardian retains its parent.
Official-parent mode was verified on this installation on 2026-10-16.
Dry-run does not write to the game or launcher installation directories.
With --launch, the game itself may write logs or other files.
Windows requests administrator permission via this EXE's embedded manifest.
No injection, hooks, patching, or shell is used.
Acquisition uses native Windows HTTPS and Authenticode; no PowerShell is needed.
If update checks fail, a verified cache can be used with a visible warning.
The CN/EndField managed parent channel is independent of game launch options.";

const HELP_ZH: &str = "endfield-direct-launch [选项] [-- 额外游戏参数...]

默认：输出启动日志，检查 Games.exe 并启动游戏，随后进入命令终端。
命令：start、restart、update、lud、preview、status、stop、language、cn、help、exit。
language 切换并保存中英文显示语言；language cn / language en 指定语言。
cn 仅回放此前日志的中文翻译，不改变默认语言。
lud 后台检查、测速、下载启动器更新，关闭启动器后应用；每次启动游戏后也会自动检查。
Tab / Shift+Tab 循环补全，Enter 执行，上下键调用历史。

  --game PATH          Endfield.exe 的绝对路径
  --cache-dir PATH     管理缓存（默认启动器旁的 endfield-cache）
  --parent PATH        指定官方父映像，跳过托管更新
  --offline            不联网，使用已验证的本地或缓存 Games.exe
  --no-local-parent    跳过本地 Games.exe 搜索，使用缓存或下载
  --direct             无官方父进程直接启动（此前游戏测试失败）
  --lang VALUE         游戏启动参数语言，默认 zh-cn；与显示语言独立
  --render MODE        default | dx11 | vk
  --sub-channel VALUE  launcher_sub_channel，默认 1；none 不传递
  --tracking-code VALUE 可选 tracking_code
  --menu               仅打开命令终端，不自动启动游戏
  --launch             检查更新并启动，不进入菜单
  --update-only        仅检查或下载 Games.exe
  --dry-run            仅预览，不联网、不下载、不创建进程
  --wait               直接模式下等待；官方父进程模式始终等待
  -h, --help           显示帮助

启动器通过 Windows 父进程属性启动游戏，官方父进程主线程保持挂起。
Ctrl+Z 关闭本会话游戏；退出或关闭启动器时游戏继续运行，隐藏守护在游戏结束后清理父进程。
启动器请求管理员权限。不注入、不 hook、不修改官方程序；游戏自身可能写入日志或状态。
启动器须放在官方安装目录之外。下载、签名与版本校验使用原生 Windows API。
更新检查失败时可回退到已验证缓存，并显示提示。";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    Launch,
    UpdateOnly,
    DryRun,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Render {
    Default,
    Dx11,
    Vk,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Options {
    game: PathBuf,
    parent: Option<PathBuf>,
    cache_dir: Option<PathBuf>,
    offline: bool,
    no_local_parent: bool,
    direct: bool,
    lang: OsString,
    render: Render,
    sub_channel: Option<OsString>,
    tracking_code: Option<OsString>,
    action: Action,
    menu: bool,
    wait: bool,
    extra: Vec<OsString>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            game: PathBuf::from(DEFAULT_GAME),
            parent: None,
            cache_dir: None,
            offline: false,
            no_local_parent: false,
            direct: false,
            lang: OsString::from("zh-cn"),
            render: Render::Default,
            sub_channel: Some(OsString::from("1")),
            tracking_code: None,
            action: Action::Launch,
            menu: false,
            wait: false,
            extra: Vec::new(),
        }
    }
}

fn contains_nul(value: &OsStr) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        value.encode_wide().any(|unit| unit == 0)
    }
    #[cfg(not(windows))]
    {
        use std::os::unix::ffi::OsStrExt;
        value.as_bytes().contains(&0)
    }
}

fn value_for(args: &mut impl Iterator<Item = OsString>, name: &str) -> Result<OsString, String> {
    let value = args.next().ok_or_else(|| format!("{name} needs a value"))?;
    if value.is_empty() || contains_nul(&value) {
        return Err(format!("{name} must have a nonempty value without NUL"));
    }
    Ok(value)
}

fn is_managed_arg(value: &OsStr) -> bool {
    let text = value.to_string_lossy();
    let flag = text.split('=').next().unwrap_or_default();
    [
        "-launcher_lang",
        "-launcher_sub_channel",
        "-tracking_code",
        "-force-d3d11",
        "-force-vulkan",
    ]
    .iter()
    .any(|reserved| flag.eq_ignore_ascii_case(reserved))
}

fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Option<Options>, String> {
    let mut result = Options::default();
    let mut chosen_action = None;
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        if contains_nul(&arg) {
            return Err("arguments must not contain NUL".into());
        }
        match arg.to_str() {
            Some("--help" | "-h") => return Ok(None),
            Some("--game") => result.game = PathBuf::from(value_for(&mut args, "--game")?),
            Some("--parent") => {
                result.parent = Some(PathBuf::from(value_for(&mut args, "--parent")?))
            }
            Some("--cache-dir") => {
                result.cache_dir = Some(PathBuf::from(value_for(&mut args, "--cache-dir")?))
            }
            Some("--offline") => result.offline = true,
            Some("--no-local-parent") => result.no_local_parent = true,
            Some("--menu") => result.menu = true,
            Some("--direct") => result.direct = true,
            Some("--lang") => result.lang = value_for(&mut args, "--lang")?,
            Some("--sub-channel") => {
                let value = value_for(&mut args, "--sub-channel")?;
                result.sub_channel = if value == "none" { None } else { Some(value) };
            }
            Some("--tracking-code") => {
                result.tracking_code = Some(value_for(&mut args, "--tracking-code")?);
            }
            Some("--render") => {
                result.render = match value_for(&mut args, "--render")?.to_str() {
                    Some("default") => Render::Default,
                    Some("dx11") => Render::Dx11,
                    Some("vk") => Render::Vk,
                    _ => return Err("--render must be default, dx11, or vk".into()),
                };
            }
            Some(flag @ ("--launch" | "--update-only" | "--dry-run")) => {
                let action = match flag {
                    "--launch" => Action::Launch,
                    "--update-only" => Action::UpdateOnly,
                    _ => Action::DryRun,
                };
                if chosen_action.is_some_and(|previous| previous != action) {
                    return Err(
                        "--launch, --update-only and --dry-run are mutually exclusive".into(),
                    );
                }
                chosen_action = Some(action);
                result.action = action;
            }
            Some("--wait") => result.wait = true,
            Some("--") => {
                for extra in args {
                    if contains_nul(&extra) {
                        return Err("extra game arguments must not contain NUL".into());
                    }
                    if is_managed_arg(&extra) {
                        return Err(format!(
                            "managed launcher/render argument must use a named option: {extra:?}"
                        ));
                    }
                    result.extra.push(extra);
                }
                break;
            }
            _ => {
                return Err(format!(
                    "unknown option {arg:?}; put extra game arguments after --"
                ));
            }
        }
    }
    if result.menu && chosen_action.is_some() {
        return Err("--menu cannot be combined with --launch, --update-only or --dry-run".into());
    }
    if result.direct && result.parent.is_some() {
        return Err("--direct cannot be combined with --parent".into());
    }
    if (result.direct || result.parent.is_some())
        && (result.offline || result.cache_dir.is_some() || result.no_local_parent)
    {
        return Err("managed cache options cannot be combined with --direct or --parent".into());
    }
    Ok(Some(result))
}

fn validate_game(game: &Path) -> Result<&Path, String> {
    if !game.is_absolute() {
        return Err("--game must be an absolute path".into());
    }
    if !game
        .file_name()
        .is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case("Endfield.exe"))
    {
        return Err("--game must name Endfield.exe".into());
    }
    let directory = game
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .ok_or_else(|| "Endfield.exe must have a parent directory".to_string())?;
    if !game.is_file() || !directory.is_dir() {
        return Err(format!(
            "game executable or parent directory does not exist: {game:?}"
        ));
    }
    Ok(directory)
}

fn prefixed(prefix: &str, value: &OsStr) -> OsString {
    let mut arg = OsString::from(prefix);
    arg.push(value);
    arg
}

fn validate_parent(parent: &Path) -> Result<(), String> {
    if !parent.is_absolute()
        || !parent.file_name().is_some_and(|name| {
            ["Games.exe", "Launcher.exe"]
                .iter()
                .any(|expected| name.to_string_lossy().eq_ignore_ascii_case(expected))
        })
    {
        return Err(
            "--parent must be an absolute path to official Games.exe or Launcher.exe".into(),
        );
    }
    if !parent.is_file() || !parent.parent().is_some_and(Path::is_dir) {
        return Err(format!(
            "parent executable or directory does not exist: {parent:?}"
        ));
    }
    Ok(())
}

fn game_args(options: &Options) -> Vec<OsString> {
    let mut args = vec![prefixed("-launcher_lang=", &options.lang)];
    if options.render == Render::Dx11 {
        args.push(OsString::from("-force-d3d11"));
    }
    if let Some(code) = &options.tracking_code {
        args.push(prefixed("-tracking_code=", code));
    }
    if let Some(channel) = &options.sub_channel {
        args.push(prefixed("-launcher_sub_channel=", channel));
    }
    args.extend(options.extra.iter().cloned());
    args
}

// Human-readable diagnostic quoting. The execution path independently builds
// a lossless UTF-16 command line without using a shell.
fn quote_arg(value: &OsStr, force: bool) -> String {
    if !force
        && !value.is_empty()
        && !value
            .to_string_lossy()
            .chars()
            .any(|ch| matches!(ch, ' ' | '\t' | '"'))
    {
        return value.to_string_lossy().into_owned();
    }
    let mut output = String::from("\"");
    let mut slashes = 0;
    for ch in value.to_string_lossy().chars() {
        if ch == '\\' {
            slashes += 1;
            continue;
        }
        if ch == '"' {
            output.push_str(&"\\".repeat(slashes * 2 + 1));
        } else {
            output.push_str(&"\\".repeat(slashes));
        }
        output.push(ch);
        slashes = 0;
    }
    output.push_str(&"\\".repeat(slashes * 2));
    output.push('"');
    output
}

fn default_cache() -> Result<PathBuf, String> {
    let executable = env::current_exe().map_err(|e| format!("cannot find launcher path: {e}"))?;
    Ok(executable
        .parent()
        .ok_or("launcher has no directory")?
        .join("endfield-cache"))
}

fn local_parents() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Ok(executable) = env::current_exe()
        && let Some(parent) = executable.parent()
    {
        paths.push(parent.join("Games.exe"));
    }
    if let Ok(directory) = env::current_dir() {
        paths.push(directory.join("Games.exe"));
    }
    // Only known version directories one level below the original installation;
    // no recursive disk scan and no installation writes.
    if let Ok(entries) = std::fs::read_dir(r"D:\yj\Hypergryph Launcher") {
        for entry in entries.take(128).flatten() {
            let name = entry.file_name();
            let text = name.to_string_lossy();
            if !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit() || b == b'.') {
                paths.push(entry.path().join("Games.exe"));
            }
        }
    }
    let mut unique = Vec::new();
    for path in paths {
        if !unique.contains(&path) {
            unique.push(path);
        }
    }
    unique
}

enum Outcome {
    Finished(bool),
    #[cfg(windows)]
    Started(Session),
}

#[cfg(windows)]
struct Session {
    child: windows::Child,
    // Keep the selected image pinned for the complete game lifetime.
    _managed_parent: Option<acquisition::ReadyParent>,
}

#[cfg(windows)]
impl Session {
    fn finish(&self) -> Result<bool, String> {
        let status = self.child.wait()?;
        if self.child.cancelled() {
            logging::info(
                "游戏已关闭，自有父进程已清理。",
                "Game stopped; owned parent cleaned up.",
            );
            return Ok(true);
        }
        logging::info(
            format!("游戏进程已退出，退出码：{status}"),
            format!("Game process exited with code: {status}"),
        );
        Ok(status == 0)
    }

    fn stop(&self) -> Result<bool, String> {
        windows::stop_game();
        self.finish()
    }
}

fn run(options: Options) -> Result<bool, String> {
    #[cfg(windows)]
    let wait = options.wait || !options.direct;
    match execute(options.clone())? {
        Outcome::Finished(success) => Ok(success),
        #[cfg(windows)]
        Outcome::Started(session) => {
            let mut update_job = None;
            menu::automatic_launcher_update(&options, &mut update_job);
            if wait {
                menu::wait_for_session(&session, &mut update_job)
            } else {
                Ok(true)
            }
        }
    }
}

fn log_executable_details(path: &Path, chinese_name: &str, english_name: &str) {
    logging::info(
        format!("{chinese_name} 路径：{}", path.display()),
        format!("{english_name} path: {}", path.display()),
    );
    #[cfg(windows)]
    let version = native_acquire::file_version(path)
        .map(|[major, minor, patch, build]| format!("{major}.{minor}.{patch}.{build}"));
    #[cfg(not(windows))]
    let version: Result<String, String> = Err("file-version reading requires Windows".into());
    match version {
        Ok(version) => logging::info(
            format!("{chinese_name} 文件版本：{version}"),
            format!("{english_name} file version: {version}"),
        ),
        Err(error) => logging::info(
            format!("{chinese_name} 文件版本：未知（{error}）；仅影响版本显示。"),
            format!("{english_name} file version: unknown ({error}); display only."),
        ),
    }
}

fn execute(mut options: Options) -> Result<Outcome, String> {
    if !options.game.is_absolute() {
        return Err("--game must be an absolute path".into());
    }
    if options.action != Action::UpdateOnly {
        validate_game(&options.game)?;
    }
    log_executable_details(&options.game, "终末地主程序 Endfield.exe", "Endfield.exe");
    let mut _managed_parent: Option<acquisition::ReadyParent> = None;
    if !options.direct {
        if let Some(parent) = &options.parent {
            validate_parent(parent)?;
            logging::info(
                "使用指定的父进程映像，跳过自动更新检查。",
                "Using explicit parent; managed update check skipped.",
            );
        } else {
            let cache = options
                .cache_dir
                .clone()
                .map(Ok)
                .unwrap_or_else(default_cache)?;
            let protected = vec![
                PathBuf::from(r"D:\yj\Hypergryph Launcher"),
                options
                    .game
                    .parent()
                    .ok_or("game has no parent directory")?
                    .to_path_buf(),
            ];
            acquisition::validate_cache(&cache, &protected)?;
            logging::info(
                format!("管理缓存：{}", cache.display()),
                format!("Managed cache: {}", cache.display()),
            );
            if options.action != Action::DryRun {
                #[cfg(windows)]
                {
                    let locals = if options.no_local_parent {
                        Vec::new()
                    } else {
                        local_parents()
                    };
                    let ready = acquisition::ensure(
                        &cache,
                        &protected,
                        &locals,
                        options.offline,
                        &mut native_acquire::NativeBackend,
                    )?;
                    logging::info(
                        format!(
                            "官方 Games.exe {} 已准备：{}",
                            ready.version,
                            ready.path.display()
                        ),
                        format!(
                            "Prepared official Games.exe {}: {}",
                            ready.version,
                            ready.path.display()
                        ),
                    );
                    options.parent = Some(ready.path.clone());
                    _managed_parent = Some(ready);
                }
                #[cfg(not(windows))]
                {
                    return Err("managed acquisition is supported only on Windows".into());
                }
            }
        }
    }
    if let Some(parent) = &options.parent {
        let name = parent
            .file_name()
            .unwrap_or(parent.as_os_str())
            .to_string_lossy();
        log_executable_details(parent, &format!("官方父映像 {name}"), &name);
    }
    if options.action == Action::UpdateOnly {
        logging::info(
            "更新检查已完成，未启动游戏进程。",
            "Update-only complete; no game process started.",
        );
        return Ok(Outcome::Finished(true));
    }
    let directory = validate_game(&options.game)?;
    let args = game_args(&options);
    logging::info(
        if options.action == Action::Launch {
            "模式：启动游戏"
        } else {
            "模式：仅预览"
        },
        if options.action == Action::Launch {
            "Mode: LAUNCH"
        } else {
            "Mode: DRY-RUN"
        },
    );
    logging::info(
        format!("工作目录：{}", directory.display()),
        format!("Working directory: {}", directory.display()),
    );
    let mut command = quote_arg(options.game.as_os_str(), true);
    for arg in &args {
        command.push(' ');
        command.push_str(&quote_arg(arg, false));
    }
    logging::info(
        format!("启动命令：{command}"),
        format!("Command: {command}"),
    );
    if options.direct {
        logging::info(
            "直接启动对比模式：此前实测未通过 SDK 启动器认证，错误码 10100。",
            "Direct comparison mode: previous real game test failed SDK launcher authentication (10100).",
        );
    } else {
        let parent = options
            .parent
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "<Games.exe>".into());
        logging::info(
            format!("官方父进程映像：{parent}（挂起创建，主线程不会恢复）"),
            format!(
                "Official parent: {parent} (created suspended; its main thread is never resumed)"
            ),
        );
        logging::info(
            "通过扩展启动信息和父进程属性创建游戏，不继承句柄。",
            "Game creation: EXTENDED_STARTUPINFO_PRESENT + PROC_THREAD_ATTRIBUTE_PARENT_PROCESS; no inherited handles.",
        );
        logging::info(
            "后台守护保持自有挂起父进程，游戏退出后清理并自行退出。",
            "The background guardian retains the owned suspended parent and cleans it up after game exit.",
        );
        logging::info(
            "Ctrl+Z 主动关闭游戏；关闭或退出 Rust 启动器时游戏继续运行。",
            "Ctrl+Z stops the game; closing or exiting the Rust launcher leaves the game running.",
        );
    }
    logging::info(
        "继承当前环境；本工具不修改安装文件。",
        "Environment: inherited. No installation files are modified by this CLI.",
    );
    logging::info(
        "SDK 检查父进程映像名称和签名；此方式已在此前本机版本验证。",
        "SDK checks the parent image name and signature. Official-parent mode was verified on this installation.",
    );
    if options.lang == "zh-cn"
        && options.render == Render::Default
        && options.sub_channel.as_deref() == Some(OsStr::new("1"))
        && options.tracking_code.is_none()
        && options.extra.is_empty()
    {
        logging::info(
            "启动参数与本机历史官方启动器进程创建日志一致。",
            "Arguments match this machine's historical official launcher process-creation logs.",
        );
    }
    if options.render == Render::Vk {
        logging::info(
            "vk：尚未确认官方 Vulkan 参数，未添加渲染参数。",
            "vk: no verified launcher Vulkan argument; no render flag was added.",
        );
    }
    if options.action == Action::DryRun {
        logging::info(
            "预览已完成，未创建进程；可用 --launch 启动游戏。",
            "Dry-run complete; no process started. Use --launch to start the game.",
        );
        if options.wait && options.direct {
            logging::info(
                "预览模式下 --wait 不生效。",
                "--wait has no effect during dry-run.",
            );
        }
        return Ok(Outcome::Finished(true));
    }
    #[cfg(windows)]
    {
        logging::info(
            "正在启动游戏；游戏自身可能写入日志或状态文件。",
            "Starting the game; the game itself may write logs or other files.",
        );
        let child = windows::launch(
            &options.game,
            directory,
            &args,
            if options.direct {
                None
            } else {
                options.parent.as_deref()
            },
        )?;
        logging::info(
            format!("游戏进程已创建，PID：{}", child.id()),
            format!("Game process created, PID: {}", child.id()),
        );
        if let Some(parent) = child.parent_id() {
            logging::info(
                format!("自有挂起父进程 PID：{parent}，保持到游戏退出。"),
                format!("Owned suspended parent PID: {parent}; kept until game exit."),
            );
        }
        Ok(Outcome::Started(Session {
            child,
            _managed_parent,
        }))
    }
    #[cfg(not(windows))]
    {
        Err("--launch is supported only on Windows".into())
    }
}

pub fn entrypoint() -> ExitCode {
    let args: Vec<_> = env::args_os().skip(1).collect();
    #[cfg(windows)]
    if args
        .first()
        .is_some_and(|arg| arg == update_worker::WORKER_FLAG)
    {
        return if update_worker::worker_entry(&args[1..]).is_ok() {
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        };
    }
    #[cfg(windows)]
    if args
        .first()
        .is_some_and(|arg| arg == windows::GUARDIAN_FLAG)
    {
        return if windows::guardian_entry(&args[1..]).is_ok() {
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        };
    }
    language::initialize();
    let no_args = args.is_empty();
    match parse(args).and_then(|options| match options {
        Some(options) if no_args || options.menu => {
            #[cfg(windows)]
            {
                menu::run(options, no_args)
            }
            #[cfg(not(windows))]
            {
                Err("interactive menu is supported only on Windows".into())
            }
        }
        Some(options) => run(options),
        None => {
            println!("{}", language::text(HELP_ZH, HELP));
            Ok(true)
        }
    }) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(error) => {
            logging::error(
                format!("发生错误，详细信息：{error}"),
                format!("Error: {error}"),
            );
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(args: &[&str]) -> Result<Options, String> {
        parse(args.iter().map(OsString::from))?.ok_or_else(|| "unexpected help".into())
    }

    #[test]
    fn default_launches_with_managed_parent_and_historical_arguments() {
        let options = parsed(&[]).unwrap();
        assert_eq!(options.action, Action::Launch);
        assert_eq!(options.parent, None);
        assert_eq!(
            game_args(&options),
            [
                OsString::from("-launcher_lang=zh-cn"),
                OsString::from("-launcher_sub_channel=1")
            ]
        );
    }

    #[test]
    fn preserves_unicode_spaces_and_argument_boundaries() {
        let options = parsed(&[
            "--lang",
            "中文 语言",
            "--sub-channel",
            "none",
            "--",
            "玩家名字",
            "a b",
            "",
        ])
        .unwrap();
        assert_eq!(
            game_args(&options),
            ["-launcher_lang=中文 语言", "玩家名字", "a b", ""]
        );
    }

    #[test]
    fn explicit_update_only_and_preview_do_not_launch() {
        assert!(parsed(&["--menu"]).unwrap().menu);
        assert!(parsed(&["--menu", "--launch"]).is_err());
        assert_eq!(
            parsed(&["--update-only"]).unwrap().action,
            Action::UpdateOnly
        );
        assert_eq!(parsed(&["--dry-run"]).unwrap().action, Action::DryRun);
        assert_eq!(
            parsed(&["--launch", "--wait"]).unwrap().action,
            Action::Launch
        );
        assert!(parsed(&["--launch", "--dry-run"]).is_err());
        assert!(parsed(&["--offline", "--parent", "X"]).is_err());
    }

    #[test]
    fn vulkan_does_not_introduce_an_unverified_flag() {
        assert_eq!(
            game_args(&parsed(&["--render", "vk"]).unwrap()),
            ["-launcher_lang=zh-cn", "-launcher_sub_channel=1"]
        );
        assert!(
            game_args(&parsed(&["--render", "dx11"]).unwrap()).contains(&"-force-d3d11".into())
        );
    }

    #[test]
    fn rejects_nul_before_process_creation() {
        for args in [
            vec!["--lang", "bad\0value"],
            vec!["--", "bad\0value"],
            vec!["--game", "bad\0value"],
        ] {
            assert!(parsed(&args).is_err());
        }
    }

    #[test]
    fn rejects_managed_flag_override_in_extra_args() {
        for arg in [
            "-launcher_lang=en-us",
            "-LAUNCHER_LANG",
            "-force-d3d11",
            "-tracking_code=x",
            "-launcher_sub_channel=x",
        ] {
            assert!(parsed(&["--", arg]).is_err());
        }
    }

    #[test]
    fn quotes_windows_trailing_backslash_and_embedded_quotes() {
        assert_eq!(quote_arg(OsStr::new("a b"), false), "\"a b\"");
        assert_eq!(quote_arg(OsStr::new("a\\"), false), "a\\");
        assert_eq!(quote_arg(OsStr::new("a b\\"), false), "\"a b\\\\\"");
        assert_eq!(quote_arg(OsStr::new("a\\\"b"), false), "\"a\\\\\\\"b\"");
        assert_eq!(quote_arg(OsStr::new(""), false), "\"\"");
        assert_eq!(quote_arg(OsStr::new("x.exe"), true), "\"x.exe\"");
    }

    #[test]
    fn rejects_missing_option_values_and_unknown_render_mode() {
        for args in [
            vec!["--game"],
            vec!["--lang", ""],
            vec!["--render", "dx12"],
            vec!["--bogus"],
        ] {
            assert!(parsed(&args).is_err());
        }
    }
}
