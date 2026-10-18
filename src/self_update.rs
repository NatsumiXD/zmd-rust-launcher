//! Rust-launcher updates are separate from official Games.exe acquisition.
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

const REPO: &str = "NatsumiXD/zmd-rust-launcher";
const EXE: &str = "endfield-direct-launch.exe";
const MAX_EXE: usize = 32 * 1024 * 1024;
const MAX_LIST: usize = 8 * 1024 * 1024;
const MAX_SUMS: usize = 64 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Route {
    name: &'static str,
    prefix: &'static str,
}
const ROUTES: [Route; 3] = [
    Route {
        name: "GitHub official",
        prefix: "",
    },
    Route {
        name: "GH-Proxy",
        prefix: "https://gh-proxy.org/",
    },
    Route {
        name: "GHFast",
        prefix: "https://ghfast.top/",
    },
];
impl Route {
    fn url(self, original: &str) -> String {
        format!("{}{original}", self.prefix)
    }
}
struct Probe {
    route: Route,
    elapsed: std::time::Duration,
    bytes: Vec<u8>,
}
struct Routing {
    order: Vec<Route>,
    sums: Vec<u8>,
}
struct Downloaded {
    bytes: Vec<u8>,
    route: Route,
}

fn rank_probes(
    results: Vec<Result<Probe, String>>,
    asset: &Asset,
    hash: &str,
) -> Result<Routing, String> {
    let mut healthy = Vec::new();
    for result in results {
        match result {
            Ok(probe) if probe.bytes.len() == asset.size && sha256(&probe.bytes) == hash => {
                let ms = probe.elapsed.as_millis();
                let speed =
                    probe.bytes.len() as f64 / 1024.0 / probe.elapsed.as_secs_f64().max(0.001);
                crate::logging::info(
                    format!(
                        "测速 {}：{ms} 毫秒，{speed:.1} KiB/s（校验文件完整传输）。",
                        probe.route.name
                    ),
                    format!(
                        "Speed test {}: {ms} ms, {speed:.1} KiB/s (complete checksum-file transfer).",
                        probe.route.name
                    ),
                );
                healthy.push(probe);
            }
            Ok(probe) => crate::logging::error(
                format!("测速 {}：校验文件不匹配，排除此通道。", probe.route.name),
                format!(
                    "Speed test {}: checksum-file mismatch; route excluded.",
                    probe.route.name
                ),
            ),
            Err(error) => crate::logging::error(
                format!("测速通道不可用：{error}"),
                format!("Speed-test route unavailable: {error}"),
            ),
        }
    }
    healthy.sort_by_key(|probe| probe.elapsed);
    if healthy.is_empty() {
        return Err("no verified official/mirror download route is available".into());
    }
    let order = healthy.iter().map(|probe| probe.route).collect();
    let best = healthy.remove(0);
    crate::logging::info(
        format!(
            "测速完成，优先使用 {}；传输失败或哈希不符时自动切换其他通过测速的通道。",
            best.route.name
        ),
        format!(
            "Speed tests complete; preferred route: {}. Failed or invalid EXE transfers fall back to the remaining verified routes.",
            best.route.name
        ),
    );
    Ok(Routing {
        order,
        sums: best.bytes,
    })
}

pub(crate) fn github_url(url: &str) -> Result<(&str, &str), String> {
    let (host, path) = url
        .strip_prefix("https://")
        .and_then(|value| value.split_once('/'))
        .ok_or("launcher update requires an absolute HTTPS URL")?;
    if !matches!(
        host,
        "api.github.com"
            | "github.com"
            | "release-assets.githubusercontent.com"
            | "objects.githubusercontent.com"
            | "github-releases.githubusercontent.com"
            | "gh-proxy.org"
            | "gh-proxy.com"
            | "ghfast.top"
    ) || url
        .bytes()
        .any(|byte| byte.is_ascii_control() || byte == b'\\')
        || url.contains('#')
    {
        return Err("launcher update URL is outside GitHub's allowed hosts".into());
    }
    // Include the slash without allocating or altering signed redirect queries.
    Ok((host, &url[url.len() - path.len() - 1..]))
}

trait Transport {
    fn get(&mut self, url: &str, limit: usize) -> Result<Vec<u8>, String>;
    fn benchmark(&mut self, asset: &Asset, hash: &str) -> Result<Routing, String> {
        let started = std::time::Instant::now();
        let bytes = self.get(&asset.browser_download_url, MAX_SUMS)?;
        rank_probes(
            vec![Ok(Probe {
                route: ROUTES[0],
                elapsed: started.elapsed(),
                bytes,
            })],
            asset,
            hash,
        )
    }
    fn fetch_exe(
        &mut self,
        asset: &Asset,
        hash: &str,
        routing: &Routing,
    ) -> Result<Downloaded, String> {
        let mut failures = Vec::new();
        for route in &routing.order {
            crate::logging::info(
                format!("正在通过 {} 下载新版启动器。", route.name),
                format!("Downloading the launcher update through {}.", route.name),
            );
            let result = self
                .get(&route.url(&asset.browser_download_url), MAX_EXE)
                .and_then(|bytes| {
                    if bytes.len() != asset.size || sha256(&bytes) != hash {
                        return Err("EXE size/SHA-256 mismatch".into());
                    }
                    check_pe(&bytes)?;
                    Ok(bytes)
                });
            match result {
                Ok(bytes) => {
                    return Ok(Downloaded {
                        bytes,
                        route: *route,
                    });
                }
                Err(error) => {
                    crate::logging::error(
                        format!(
                            "通道 {} 下载失败：{error}；尝试下一条通过测速的通道。",
                            route.name
                        ),
                        format!(
                            "Download route {} failed: {error}; trying the next verified route.",
                            route.name
                        ),
                    );
                    failures.push(format!("{}: {error}", route.name));
                }
            }
        }
        Err(format!(
            "all benchmarked EXE routes failed; original preserved: {}",
            failures.join("; ")
        ))
    }
}

#[cfg(windows)]
struct NativeTransport;
#[cfg(windows)]
impl Transport for NativeTransport {
    fn get(&mut self, url: &str, limit: usize) -> Result<Vec<u8>, String> {
        use std::io::Read;
        let mut bytes = Vec::new();
        crate::native_acquire::Response::github(url)?
            .take(limit as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| format!("GitHub response read failed: {error}"))?;
        if bytes.len() > limit {
            return Err("GitHub response exceeds the launcher-update size limit".into());
        }
        Ok(bytes)
    }
    fn benchmark(&mut self, asset: &Asset, hash: &str) -> Result<Routing, String> {
        use std::io::Read;
        crate::logging::info(
            "先并行测速 GitHub 官方、GH-Proxy 和 GHFast；使用已知官方哈希的校验文件，测速完成后才下载和安装新版 EXE。",
            "Benchmarking GitHub official, GH-Proxy and GHFast in parallel using the checksum file and its official digest; EXE download/install starts after testing.",
        );
        let results = std::thread::scope(|scope| {
            let workers: Vec<_> = ROUTES
                .into_iter()
                .map(|route| {
                    scope.spawn(move || {
                        let started = std::time::Instant::now();
                        let result = (|| {
                            let mut bytes = Vec::new();
                            crate::native_acquire::Response::github_probe(
                                &route.url(&asset.browser_download_url),
                            )?
                            .take(MAX_SUMS as u64 + 1)
                            .read_to_end(&mut bytes)
                            .map_err(|error| error.to_string())?;
                            if bytes.len() > MAX_SUMS {
                                return Err("checksum probe exceeds size limit".into());
                            }
                            Ok(Probe {
                                route,
                                elapsed: started.elapsed(),
                                bytes,
                            })
                        })();
                        result.map_err(|error: String| format!("{}: {error}", route.name))
                    })
                })
                .collect();
            workers
                .into_iter()
                .map(|worker| {
                    worker
                        .join()
                        .unwrap_or_else(|_| Err("speed-test worker failed".into()))
                })
                .collect()
        });
        rank_probes(results, asset, hash)
    }
}

#[derive(Clone, Deserialize)]
struct Asset {
    name: String,
    state: String,
    size: usize,
    digest: Option<String>,
    browser_download_url: String,
}
#[derive(Deserialize)]
struct Release {
    tag_name: String,
    target_commitish: String,
    draft: bool,
    assets: Vec<Asset>,
}
type ReleaseKey = ([u64; 3], u64, u64);

fn version(text: &str) -> Option<[u64; 3]> {
    let parts: Vec<_> = text.split('.').collect();
    if parts.len() != 3 {
        return None;
    }
    let mut result = [0; 3];
    for (index, part) in parts.iter().enumerate() {
        if part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        result[index] = part.parse().ok()?;
    }
    Some(result)
}
fn build(text: &str) -> Option<(u64, u64)> {
    let (run, attempt) = text.split_once('.')?;
    if [run, attempt]
        .iter()
        .any(|value| value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()))
    {
        return None;
    }
    Some((run.parse().ok()?, attempt.parse().ok()?))
}
fn key(tag: &str) -> Option<ReleaseKey> {
    let (release, suffix) = tag.strip_prefix("pre-v")?.rsplit_once('-')?;
    let (run, attempt) = build(suffix)?;
    Some((version(release)?, run, attempt))
}
fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn valid_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
fn asset<'a>(
    release: &'a Release,
    name: &str,
    limit: usize,
) -> Result<(&'a Asset, &'a str), String> {
    let entries: Vec<_> = release
        .assets
        .iter()
        .filter(|asset| asset.name == name)
        .collect();
    if entries.len() != 1 {
        return Err(format!("release must contain exactly one {name}"));
    }
    let asset = entries[0];
    let hash = asset
        .digest
        .as_deref()
        .and_then(|text| text.strip_prefix("sha256:"))
        .filter(|hash| valid_hash(hash))
        .ok_or("release asset lacks a valid SHA-256 digest")?;
    let expected = format!(
        "https://github.com/{REPO}/releases/download/{}/{name}",
        release.tag_name
    );
    if asset.state != "uploaded"
        || asset.size == 0
        || asset.size > limit
        || asset.browser_download_url != expected
    {
        return Err(format!("invalid launcher release asset: {name}"));
    }
    Ok((asset, hash))
}
fn latest(bytes: &[u8]) -> Result<Release, String> {
    if bytes.len() > MAX_LIST {
        return Err("release list exceeds size limit".into());
    }
    let releases: Vec<Release> = serde_json::from_slice(bytes)
        .map_err(|error| format!("invalid GitHub release list: {error}"))?;
    let release = releases
        .into_iter()
        .filter(|release| !release.draft && key(&release.tag_name).is_some())
        .max_by_key(|release| key(&release.tag_name).unwrap())
        .ok_or("no published launcher prerelease found")?;
    if release.target_commitish.len() != 40
        || !release
            .target_commitish
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return Err("release target must identify an exact source commit".into());
    }
    Ok(release)
}
fn newer(
    release: &Release,
    current_version: &str,
    current_commit: &str,
    current_build: &str,
) -> Result<bool, String> {
    let remote = key(&release.tag_name).ok_or("invalid release tag")?;
    let local = version(current_version).ok_or("invalid current launcher version")?;
    if remote.0 != local {
        return Ok(remote.0 > local);
    }
    if let Some(local_build) = build(current_build) {
        return Ok((remote.1, remote.2) > local_build);
    }
    Ok(release.target_commitish != current_commit)
}
fn check_sums(bytes: &[u8], expected: &str) -> Result<(), String> {
    let text = std::str::from_utf8(bytes).map_err(|_| "SHA256SUMS.txt is not UTF-8")?;
    let mut hashes = text
        .lines()
        .filter_map(|line| line.split_once("  "))
        .filter(|(_, name)| *name == EXE)
        .map(|(hash, _)| hash);
    if hashes.next() != Some(expected) || hashes.next().is_some() {
        return Err("EXE digest does not match SHA256SUMS.txt".into());
    }
    Ok(())
}
fn check_pe(bytes: &[u8]) -> Result<(), String> {
    let offset = bytes
        .get(0x3c..0x40)
        .map(|part| u32::from_le_bytes(part.try_into().unwrap()) as usize)
        .ok_or("downloaded launcher has no PE header")?;
    let end = offset.checked_add(6).ok_or("invalid PE offset")?;
    if bytes.get(..2) != Some(b"MZ") || bytes.get(offset..end) != Some(b"PE\0\0\x64\x86") {
        return Err("downloaded launcher is not a Windows x64 PE executable".into());
    }
    Ok(())
}

#[derive(Clone)]
pub(crate) struct Prepared {
    pub target: PathBuf,
    pub tag: String,
    pub backup: PathBuf,
    staged: PathBuf,
    old_hash: String,
    new_hash: String,
    protected: Vec<PathBuf>,
}

// A two-step same-volume rename keeps the loaded old image alive, while new
// launches use the new file. If the second step fails, restore the original.
fn swap(target: &Path, staged: &Path, backup: &Path) -> Result<(), String> {
    if backup.exists() {
        return Err("launcher backup already exists".into());
    }
    fs::rename(target, backup)
        .map_err(|error| format!("cannot move old launcher to backup: {error}"))?;
    if let Err(error) = fs::rename(staged, target) {
        return match fs::rename(backup, target) {
            Ok(()) => Err(format!(
                "launcher replacement failed; original restored: {error}"
            )),
            Err(rollback) => Err(format!(
                "replacement failed: {error}; restore {} manually ({rollback})",
                backup.display()
            )),
        };
    }
    Ok(())
}

fn prepare(
    target: &Path,
    protected: &[PathBuf],
    current_version: &str,
    current_commit: &str,
    current_build: &str,
    transport: &mut impl Transport,
    cached: Option<&Prepared>,
) -> Result<Option<Prepared>, String> {
    crate::acquisition::validate_cache(target, protected)?;
    crate::logging::info(
        format!("检查 Rust 启动器更新：当前 {current_version}，构建 {current_build}。"),
        format!(
            "Checking Rust launcher updates: current {current_version}, build {current_build}."
        ),
    );
    let releases = transport.get(
        &format!("https://api.github.com/repos/{REPO}/releases?per_page=100"),
        MAX_LIST,
    )?;
    let release = latest(&releases)?;
    if !newer(&release, current_version, current_commit, current_build)? {
        crate::logging::info(
            "Rust 启动器已是最新版本，未修改文件。",
            "Rust launcher is up to date; no files changed.",
        );
        return Ok(None);
    }
    let (exe, expected) = asset(&release, EXE, MAX_EXE)?;
    let (sums, sums_hash) = asset(&release, "SHA256SUMS.txt", MAX_SUMS)?;
    let root = target
        .parent()
        .ok_or("launcher has no parent directory")?
        .join("launcher-update");
    crate::acquisition::validate_cache(&root, protected)?;
    let routing = transport.benchmark(sums, sums_hash)?;
    check_sums(&routing.sums, expected)?;
    let old_bytes = fs::read(target).map_err(|error| error.to_string())?;
    let old_hash = sha256(&old_bytes);
    if old_hash == expected {
        crate::logging::info(
            "本地 EXE 已与最新发布一致。",
            "Local EXE already matches the latest release.",
        );
        return Ok(None);
    }
    if let Some(prepared) = cached
        && prepared.new_hash == expected
        && prepared.old_hash == old_hash
        && fs::read(&prepared.staged).is_ok_and(|bytes| sha256(&bytes) == expected)
    {
        crate::logging::info(
            "新版已下载并校验，继续等待启动器关闭后应用。",
            "Verified update is already staged; it will be applied after this launcher closes.",
        );
        return Ok(Some(prepared.clone()));
    }
    crate::logging::info(
        format!(
            "发现启动器更新 {}，自动下载并校验 SHA-256。",
            release.tag_name
        ),
        format!(
            "Launcher update found: {}; downloading and checking SHA-256.",
            release.tag_name
        ),
    );
    let downloaded = transport.fetch_exe(exe, expected, &routing)?;
    let bytes = downloaded.bytes;
    if bytes.len() != exe.size || bytes.len() > MAX_EXE || sha256(&bytes) != expected {
        return Err("launcher EXE SHA-256/size mismatch; original preserved".into());
    }
    check_pe(&bytes)?;
    // No installation writes occur until all speed tests and EXE validation pass.
    fs::create_dir_all(&root).map_err(|error| error.to_string())?;
    let lock_path = root.join("update.lock");
    crate::acquisition::validate_cache(&lock_path, protected)?;
    let mut lock_options = OpenOptions::new();
    lock_options
        .read(true)
        .write(true)
        .create(true)
        .truncate(false);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        lock_options.share_mode(0);
    }
    let _lock = lock_options.open(&lock_path).map_err(|error| {
        format!("another update may be running; cannot lock launcher update: {error}")
    })?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_nanos();
    let job = root.join(format!("{}-{stamp}", std::process::id()));
    fs::create_dir(&job).map_err(|error| error.to_string())?;
    let staged = job.join("new.exe");
    let backup = job.join("previous.exe");
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&staged)
        .map_err(|error| error.to_string())?;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|error| error.to_string())?;
    drop(file);
    let receipt = serde_json::json!({"tag":release.tag_name,"commit":release.target_commitish,"target":target,"backup":backup,"old_sha256":old_hash,"new_sha256":expected,"download_route":downloaded.route.name});
    fs::write(
        job.join("receipt.json"),
        serde_json::to_vec_pretty(&receipt).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    crate::logging::info(
        format!(
            "启动器更新已下载并校验：{}；关闭启动器后自动应用，旧版将备份到 {}。",
            release.tag_name,
            backup.display()
        ),
        format!(
            "Launcher update downloaded and verified: {}; it will be applied after the launcher closes, with the previous EXE backed up at {}.",
            release.tag_name,
            backup.display()
        ),
    );
    Ok(Some(Prepared {
        target: target.into(),
        tag: release.tag_name.clone(),
        backup,
        staged,
        old_hash,
        new_hash: expected.into(),
        protected: protected.to_vec(),
    }))
}

pub(crate) fn apply(prepared: &Prepared) -> Result<(), String> {
    crate::acquisition::validate_cache(&prepared.target, &prepared.protected)?;
    crate::acquisition::validate_cache(&prepared.staged, &prepared.protected)?;
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(0);
    }
    let lock_path = prepared
        .staged
        .parent()
        .and_then(Path::parent)
        .ok_or("invalid staged path")?
        .join("update.lock");
    let _lock = options
        .open(lock_path)
        .map_err(|error| format!("cannot lock update application: {error}"))?;
    let new_bytes = fs::read(&prepared.staged).map_err(|error| error.to_string())?;
    if sha256(&new_bytes) != prepared.new_hash {
        return Err("staged EXE changed; update not applied".into());
    }
    check_pe(&new_bytes)?;
    let current = sha256(&fs::read(&prepared.target).map_err(|error| error.to_string())?);
    if current == prepared.new_hash {
        return Ok(());
    }
    if current != prepared.old_hash {
        return Err("launcher changed since download; refusing to overwrite it".into());
    }
    swap(&prepared.target, &prepared.staged, &prepared.backup)?;
    crate::logging::info(
        format!(
            "启动器已关闭，更新 {} 已应用；旧版备份：{}。",
            prepared.tag,
            prepared.backup.display()
        ),
        format!(
            "Launcher closed; update {} applied. Previous EXE: {}.",
            prepared.tag,
            prepared.backup.display()
        ),
    );
    Ok(())
}

#[cfg(test)]
fn perform(
    target: &Path,
    protected: &[PathBuf],
    version: &str,
    commit: &str,
    build: &str,
    transport: &mut impl Transport,
) -> Result<Option<Prepared>, String> {
    let prepared = prepare(target, protected, version, commit, build, transport, None)?;
    if let Some(prepared) = &prepared {
        apply(prepared)?;
    }
    Ok(prepared)
}

#[cfg(windows)]
pub(crate) fn prepare_native(
    target: &Path,
    protected: &[PathBuf],
    version: &str,
    commit: &str,
    build: &str,
    cached: Option<&Prepared>,
) -> Result<Option<Prepared>, String> {
    prepare(
        target,
        protected,
        version,
        commit,
        build,
        &mut NativeTransport,
        cached,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn directory() -> PathBuf {
        static COUNT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("artifacts/self-update-tests")
            .join(format!(
                "{}-{stamp}-{}",
                std::process::id(),
                COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ));
        fs::create_dir_all(&path).unwrap();
        path
    }
    struct Mock {
        release: serde_json::Value,
        exe: Vec<u8>,
        sums: Vec<u8>,
        requests: Vec<String>,
    }
    impl Mock {
        fn new() -> Self {
            let mut exe = vec![0; 256];
            exe[..2].copy_from_slice(b"MZ");
            exe[0x3c..0x40].copy_from_slice(&0x80u32.to_le_bytes());
            exe[0x80..0x86].copy_from_slice(b"PE\0\0\x64\x86");
            let sums = format!("{}  {EXE}\n", sha256(&exe)).into_bytes();
            let release = serde_json::json!({
                "tag_name":"pre-v0.3.0-20.1", "target_commitish":"b".repeat(40),"draft":false,
                "assets":[
                    {"name":EXE,"state":"uploaded","size":exe.len(),"digest":format!("sha256:{}",sha256(&exe)),"browser_download_url":format!("https://github.com/{REPO}/releases/download/pre-v0.3.0-20.1/{EXE}")},
                    {"name":"SHA256SUMS.txt","state":"uploaded","size":sums.len(),"digest":format!("sha256:{}",sha256(&sums)),"browser_download_url":format!("https://github.com/{REPO}/releases/download/pre-v0.3.0-20.1/SHA256SUMS.txt")}
                ]
            });
            Self {
                release,
                exe,
                sums,
                requests: Vec::new(),
            }
        }
        fn refresh_digests(&mut self) {
            self.release["assets"][0]["digest"] = format!("sha256:{}", sha256(&self.exe)).into();
            self.release["assets"][0]["size"] = self.exe.len().into();
            self.release["assets"][1]["digest"] = format!("sha256:{}", sha256(&self.sums)).into();
            self.release["assets"][1]["size"] = self.sums.len().into();
        }
    }
    impl Transport for Mock {
        fn get(&mut self, url: &str, _limit: usize) -> Result<Vec<u8>, String> {
            self.requests.push(url.into());
            if url.starts_with("https://api.github.com/") {
                Ok(serde_json::to_vec(&vec![&self.release]).unwrap())
            } else if url.ends_with("/SHA256SUMS.txt") {
                Ok(self.sums.clone())
            } else {
                Ok(self.exe.clone())
            }
        }
    }
    fn run(mock: &mut Mock, target: &Path) -> Result<Option<Prepared>, String> {
        perform(target, &[], "0.2.0", &"a".repeat(40), "10.1", mock)
    }

    #[test]
    fn release_ordering_includes_prereleases_and_prevents_downgrades() {
        let mock = Mock::new();
        let mut draft = mock.release.clone();
        draft["draft"] = true.into();
        draft["tag_name"] = "pre-v9.0.0-99.1".into();
        let mut previous = mock.release.clone();
        previous["tag_name"] = "pre-v0.3.0-19.9".into();
        let release =
            latest(&serde_json::to_vec(&vec![previous, draft, mock.release]).unwrap()).unwrap();
        assert_eq!(release.tag_name, "pre-v0.3.0-20.1");
        assert!(newer(&release, "0.2.0", "local", "999.1").unwrap());
        assert!(!newer(&release, "0.4.0", "local", "1.1").unwrap());
        assert!(!newer(&release, "0.3.0", "local", "21.1").unwrap());
        assert!(!newer(&release, "0.3.0", "local", "20.1").unwrap());
        assert!(newer(&release, "0.3.0", "local", "20.0").unwrap());
        assert!(!newer(&release, "0.3.0", &release.target_commitish, "local").unwrap());
        assert!(key("pre-v0.3.0-../../bad").is_none());
    }

    #[test]
    fn preparing_updates_defers_replacement_and_reuses_verified_downloads() {
        let target = directory().join(EXE);
        fs::write(&target, b"original").unwrap();
        let mut mock = Mock::new();
        let prepared = prepare(&target, &[], "0.2.0", "local", "local", &mut mock, None)
            .unwrap()
            .unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"original");
        assert!(!prepared.backup.exists());
        let count = mock.requests.len();
        let cached = prepare(
            &target,
            &[],
            "0.2.0",
            "local",
            "local",
            &mut mock,
            Some(&prepared),
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            mock.requests.len() - count,
            2,
            "cached EXE must not be downloaded again"
        );
        assert_eq!(cached.staged, prepared.staged);
        fs::write(&target, b"another launcher update").unwrap();
        assert!(
            apply(&prepared).is_err(),
            "do not overwrite a changed target"
        );
        assert_eq!(fs::read(&target).unwrap(), b"another launcher update");
    }

    #[test]
    fn verified_update_keeps_previous_exe_and_receipt() {
        let target = directory().join(EXE);
        fs::write(&target, b"old launcher").unwrap();
        let mut mock = Mock::new();
        let installed = run(&mut mock, &target).unwrap().unwrap();
        assert_eq!(fs::read(&target).unwrap(), mock.exe);
        assert_eq!(fs::read(&installed.backup).unwrap(), b"old launcher");
        let receipt: serde_json::Value = serde_json::from_slice(
            &fs::read(installed.backup.with_file_name("receipt.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(receipt["new_sha256"], sha256(&mock.exe));
        assert_eq!(mock.requests.len(), 3);
        let mut identical = Mock::new();
        assert!(run(&mut identical, &target).unwrap().is_none());
        assert_eq!(
            identical.requests.len(),
            2,
            "identical EXE must not be downloaded"
        );
    }

    #[test]
    fn corrupt_untrusted_or_ambiguous_assets_preserve_current_exe() {
        for failure in 0..7 {
            let target = directory().join(EXE);
            fs::write(&target, b"original").unwrap();
            let mut mock = Mock::new();
            match failure {
                0 => mock.exe[0] ^= 1,
                1 => mock.sums[0] ^= 1,
                2 => {
                    mock.release["assets"][0]["browser_download_url"] =
                        "https://evil.example/launcher.exe".into()
                }
                3 => mock.release["assets"][0]["digest"] = serde_json::Value::Null,
                4 => {
                    let extra = mock.release["assets"][0].clone();
                    mock.release["assets"].as_array_mut().unwrap().push(extra);
                }
                5 => {
                    mock.exe[0] = 0;
                    mock.sums = format!("{}  {EXE}\n", sha256(&mock.exe)).into_bytes();
                    mock.refresh_digests();
                }
                _ => {
                    mock.sums = format!(
                        "{}  {EXE}\n{}  {EXE}\n",
                        sha256(&mock.exe),
                        sha256(&mock.exe)
                    )
                    .into_bytes();
                    mock.refresh_digests();
                }
            }
            assert!(
                run(&mut mock, &target).is_err(),
                "failure {failure} was accepted"
            );
            assert_eq!(fs::read(&target).unwrap(), b"original");
        }
    }

    #[test]
    fn update_checks_do_not_write_installation_or_downgrade_current_build() {
        let root = directory();
        let target = root.join(EXE);
        fs::write(&target, b"original").unwrap();
        let mut mock = Mock::new();
        assert!(
            perform(
                &target,
                std::slice::from_ref(&root),
                "0.2.0",
                "local",
                "local",
                &mut mock
            )
            .is_err()
        );
        assert!(mock.requests.is_empty());
        assert!(
            perform(&target, &[], "0.4.0", "local", "local", &mut mock)
                .unwrap()
                .is_none()
        );
        assert_eq!(mock.requests.len(), 1);
        assert!(!root.join("launcher-update").exists());
        assert_eq!(fs::read(target).unwrap(), b"original");
    }

    #[test]
    fn failed_replacement_restores_the_original_exe() {
        let root = directory();
        let target = root.join(EXE);
        let backup = root.join("previous.exe");
        fs::write(&target, b"original").unwrap();
        let error = swap(&target, &root.join("missing.exe"), &backup).unwrap_err();
        assert!(error.contains("original restored"));
        assert_eq!(fs::read(&target).unwrap(), b"original");
        assert!(!backup.exists());
        fs::write(&backup, b"existing backup").unwrap();
        assert!(swap(&target, &root.join("missing.exe"), &backup).is_err());
        assert_eq!(fs::read(&backup).unwrap(), b"existing backup");
    }

    #[test]
    fn speed_tests_rank_only_verified_responses_and_keep_official_as_a_candidate() {
        let mock = Mock::new();
        let release: Release = serde_json::from_value(mock.release.clone()).unwrap();
        let (asset, hash) = asset(&release, "SHA256SUMS.txt", MAX_SUMS).unwrap();
        let routing = rank_probes(
            vec![
                Ok(Probe {
                    route: ROUTES[0],
                    elapsed: std::time::Duration::from_millis(200),
                    bytes: mock.sums.clone(),
                }),
                Ok(Probe {
                    route: ROUTES[1],
                    elapsed: std::time::Duration::from_millis(1),
                    bytes: b"fake-fast-response".to_vec(),
                }),
                Ok(Probe {
                    route: ROUTES[2],
                    elapsed: std::time::Duration::from_millis(100),
                    bytes: mock.sums.clone(),
                }),
            ],
            asset,
            hash,
        )
        .unwrap();
        assert_eq!(routing.order, vec![ROUTES[2], ROUTES[0]]);
        assert_eq!(routing.sums, mock.sums);
        assert!(rank_probes(vec![Err("offline".into())], asset, hash).is_err());
    }

    #[test]
    fn bad_mirror_exe_falls_back_to_the_next_benchmarked_route() {
        struct Failover(Mock);
        impl Transport for Failover {
            fn get(&mut self, url: &str, limit: usize) -> Result<Vec<u8>, String> {
                if url.starts_with(ROUTES[2].prefix) {
                    return Ok(b"tampered mirror executable".to_vec());
                }
                self.0.get(url, limit)
            }
        }
        let mut transport = Failover(Mock::new());
        let release: Release = serde_json::from_value(transport.0.release.clone()).unwrap();
        let (exe, hash) = asset(&release, EXE, MAX_EXE).unwrap();
        let routing = Routing {
            order: vec![ROUTES[2], ROUTES[0]],
            sums: Vec::new(),
        };
        let downloaded = transport.fetch_exe(exe, hash, &routing).unwrap();
        assert_eq!(downloaded.route, ROUTES[0]);
        assert_eq!(downloaded.bytes, transport.0.exe);
        assert_eq!(transport.0.requests.len(), 1);
        assert_eq!(
            ROUTES[1].url(&exe.browser_download_url),
            format!("https://gh-proxy.org/{}", exe.browser_download_url)
        );
    }

    #[test]
    fn github_downloads_require_https_and_known_hosts() {
        assert_eq!(
            github_url("https://release-assets.githubusercontent.com/x?signature=a%2Fb").unwrap(),
            ("release-assets.githubusercontent.com", "/x?signature=a%2Fb")
        );
        for url in [
            "http://github.com/x",
            "https://github.com.evil.test/x",
            "https://user@github.com/x",
            "https://github.com:444/x",
            "https://github.com/x\r\nHeader: bad",
            "https://github.com/\\evil",
            "https://github.com/x#fragment",
        ] {
            assert!(github_url(url).is_err());
        }
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "network and workspace-only running-image update probe; build process-probe first"]
    fn github_workspace_running_launcher_update_probe() {
        let target = directory().join(EXE);
        fs::copy(
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/debug/process-probe.exe"),
            &target,
        )
        .unwrap();
        let original = fs::read(&target).unwrap();
        let mut process = std::process::Command::new(&target)
            .arg("--stay-alive")
            .spawn()
            .unwrap();
        let result = perform(
            &target,
            &[],
            "0.0.0",
            "local",
            "local",
            &mut NativeTransport,
        );
        let still_running = process.try_wait().unwrap().is_none();
        let _ = process.kill();
        let _ = process.wait();
        let installed = result.unwrap().unwrap();
        assert!(
            still_running,
            "installing a launcher update must not stop the loaded old image"
        );
        assert_eq!(fs::read(&installed.backup).unwrap(), original);
        check_pe(&fs::read(&target).unwrap()).unwrap();
        fs::write(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("analysis/launcher-self-update-probe.json"),
            serde_json::to_vec_pretty(&serde_json::json!({"tag":installed.tag,"target":target,"backup":installed.backup,"old_process_kept_running":still_running,"new_exe_executed":false,"sha256":sha256(&fs::read(&target).unwrap())})).unwrap()).unwrap();
    }
}
