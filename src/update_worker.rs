//! A trusted copy of the current launcher downloads updates and waits on an
//! inherited owner-process handle before replacing the visible launcher's EXE.
use serde::{Deserialize, Serialize};
use std::ffi::OsString;
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

pub(crate) const WORKER_FLAG: &str = "--internal-launcher-update";
#[derive(Serialize, Deserialize)]
struct Config {
    target: PathBuf,
    protected: Vec<PathBuf>,
    version: String,
    commit: String,
    build: String,
}

pub(crate) fn create_layout(
    target: &Path,
    helper_source: &Path,
    protected: &[PathBuf],
    version: &str,
    commit: &str,
    build: &str,
) -> Result<PathBuf, String> {
    if target.file_name().and_then(|name| name.to_str()) != Some("endfield-direct-launch.exe") {
        return Err(
            "automatic updates require the launcher filename endfield-direct-launch.exe".into(),
        );
    }
    crate::acquisition::validate_cache(target, protected)?;
    let root = target
        .parent()
        .ok_or("launcher has no directory")?
        .join("launcher-update");
    crate::acquisition::validate_cache(&root, protected)?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_nanos();
    let job = root.join(format!("worker-{}-{stamp}", std::process::id()));
    fs::create_dir_all(&job).map_err(|error| error.to_string())?;
    let config = Config {
        target: target.into(),
        protected: protected.into(),
        version: version.into(),
        commit: commit.into(),
        build: build.into(),
    };
    fs::write(
        job.join("config.json"),
        serde_json::to_vec(&config).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    fs::write(job.join("request.txt"), b"1\n").map_err(|error| error.to_string())?;
    let helper = job.join("updater.exe");
    fs::copy(helper_source, &helper)
        .map_err(|error| format!("cannot copy trusted update helper: {error}"))?;
    Ok(helper)
}

pub(crate) struct UpdateJob {
    worker: crate::windows::UpdateWorker,
    directory: PathBuf,
    request: u64,
    offset: u64,
    partial: Vec<u8>,
    journal_failed: bool,
}
impl UpdateJob {
    pub(crate) fn start_or_request(
        options: &crate::Options,
        active: &mut Option<Self>,
    ) -> Result<(), String> {
        if let Some(job) = active.as_mut().filter(|job| job.worker.is_running()) {
            job.request = job
                .request
                .checked_add(1)
                .ok_or("update request counter overflow")?;
            fs::write(
                job.directory.join("request.txt"),
                format!("{}\n", job.request),
            )
            .map_err(|error| error.to_string())?;
        } else {
            let target = std::env::current_exe().map_err(|error| error.to_string())?;
            let protected = vec![
                PathBuf::from(r"D:\yj\Hypergryph Launcher"),
                options.game.parent().ok_or("game has no directory")?.into(),
            ];
            let helper = create_layout(
                &target,
                &target,
                &protected,
                env!("CARGO_PKG_VERSION"),
                env!("LAUNCHER_COMMIT"),
                env!("LAUNCHER_BUILD"),
            )?;
            let worker = crate::windows::spawn_update_worker(&helper)?;
            *active = Some(Self {
                worker,
                directory: helper.parent().unwrap().into(),
                request: 1,
                offset: 0,
                partial: Vec::new(),
                journal_failed: false,
            });
        }
        crate::logging::info(
            "已安排后台启动器更新检查：先测速、下载并校验，关闭当前启动器后才应用；游戏继续运行。",
            "Background launcher update check requested: benchmark, download and verify first; apply only after this launcher closes. The game keeps running.",
        );
        Ok(())
    }

    pub(crate) fn poll(&mut self) -> Result<Vec<crate::logging::JournalRecord>, String> {
        if self.journal_failed {
            return Ok(Vec::new());
        }
        let result = self.read_records();
        if result.is_err() {
            self.journal_failed = true;
        }
        result
    }
    fn read_records(&mut self) -> Result<Vec<crate::logging::JournalRecord>, String> {
        let path = self.directory.join("events.jsonl");
        let mut file = match fs::File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error.to_string()),
        };
        if file.metadata().map_err(|error| error.to_string())?.len() > 8 * 1024 * 1024 {
            return Err("update journal exceeds size limit".into());
        }
        file.seek(SeekFrom::Start(self.offset))
            .map_err(|error| error.to_string())?;
        let mut bytes = Vec::new();
        file.take(65536)
            .read_to_end(&mut bytes)
            .map_err(|error| error.to_string())?;
        self.offset += bytes.len() as u64;
        self.partial.extend(bytes);
        let mut records = Vec::new();
        while let Some(end) = self.partial.iter().position(|byte| *byte == b'\n') {
            let line: Vec<_> = self.partial.drain(..=end).collect();
            records.push(
                serde_json::from_slice(&line)
                    .map_err(|error| format!("invalid update journal: {error}"))?,
            );
        }
        if self.partial.len() > 65536 {
            return Err("update journal record exceeds size limit".into());
        }
        Ok(records)
    }
}

fn state(directory: &Path, phase: &str, tag: Option<&str>) -> Result<(), String> {
    fs::write(
        directory.join("state.json"),
        serde_json::to_vec(&serde_json::json!({"phase":phase,"tag":tag}))
            .map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())
}
fn requested(directory: &Path) -> Option<u64> {
    let bytes = fs::read(directory.join("request.txt")).ok()?;
    if bytes.len() > 32 {
        return None;
    }
    std::str::from_utf8(&bytes).ok()?.trim().parse().ok()
}

pub(crate) fn worker_entry(args: &[OsString]) -> Result<(), String> {
    let owner = crate::windows::UpdateOwner::inherited(args)?;
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let directory = executable.parent().ok_or("worker has no directory")?;
    let bytes = fs::read(directory.join("config.json")).map_err(|error| error.to_string())?;
    if bytes.len() > 65536 {
        return Err("update-worker config exceeds size limit".into());
    }
    let mut config: Config = serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
    config
        .protected
        .push(PathBuf::from(r"D:\yj\Hypergryph Launcher"));
    crate::acquisition::validate_cache(&config.target, &config.protected)?;
    crate::acquisition::validate_cache(directory, &config.protected)?;
    let tool_root = directory
        .parent()
        .filter(|root| {
            root.file_name()
                .is_some_and(|name| name == "launcher-update")
        })
        .and_then(Path::parent)
        .ok_or("invalid update-worker directory")?;
    if executable
        .file_name()
        .is_none_or(|name| name != "updater.exe")
        || config
            .target
            .file_name()
            .is_none_or(|name| name != "endfield-direct-launch.exe")
        || fs::canonicalize(config.target.parent().ok_or("target has no directory")?)
            .map_err(|error| error.to_string())?
            != fs::canonicalize(tool_root).map_err(|error| error.to_string())?
    {
        return Err("worker target is outside its original launcher directory".into());
    }
    crate::logging::attach_journal(&directory.join("events.jsonl"))
        .map_err(|error| error.to_string())?;
    state(directory, "ready", None)?;
    owner.acknowledge()?;
    let mut handled = 0;
    let mut pending: Option<crate::self_update::Prepared> = None;
    loop {
        if let Some(request) = requested(directory).filter(|request| *request > handled) {
            handled = request;
            state(
                directory,
                "checking",
                pending.as_ref().map(|update| update.tag.as_str()),
            )?;
            match crate::self_update::prepare_native(
                &config.target,
                &config.protected,
                &config.version,
                &config.commit,
                &config.build,
                pending.as_ref(),
            ) {
                Ok(update) => {
                    pending = update;
                    state(
                        directory,
                        if pending.is_some() {
                            "staged"
                        } else {
                            "current"
                        },
                        pending.as_ref().map(|update| update.tag.as_str()),
                    )?;
                }
                Err(error) => {
                    crate::logging::error(
                        format!("后台启动器更新检查失败：{error}；已存在的有效下载仍保留。"),
                        format!(
                            "Background launcher update check failed: {error}; any previously verified staged update is preserved."
                        ),
                    );
                    state(
                        directory,
                        "error",
                        pending.as_ref().map(|update| update.tag.as_str()),
                    )?;
                }
            }
        }
        if owner.exited(500)? {
            if requested(directory).is_some_and(|request| request > handled) {
                continue;
            }
            if let Some(update) = pending {
                if let Err(error) = crate::self_update::apply(&update) {
                    crate::logging::error(
                        format!("关闭后的启动器更新未完成：{error}"),
                        format!("Post-exit launcher update failed: {error}"),
                    );
                    state(directory, "failed", Some(&update.tag))?;
                    return Err(error);
                }
                state(directory, "applied", Some(&update.tag))?;
            } else {
                state(directory, "finished", None)?;
            }
            return Ok(());
        }
    }
}
