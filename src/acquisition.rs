//! Managed official parent cache. Releases are immutable; only current.json switches.
use md5::Md5;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub const PUBLISHER: &str = "Shanghai Hypergryph Network Technology Co., Ltd.";
const MAX_PACKAGE: u64 = 512 * 1024 * 1024;
const MAX_IMAGE: u64 = 128 * 1024 * 1024;

pub trait Backend {
    fn metadata(&mut self, version: &str) -> Result<Vec<u8>, String>;
    fn download(&mut self, url: &str, output: &mut dyn Write, size: u64) -> Result<(), String>;
    fn verify(&mut self, path: &Path) -> Result<String, String>;
    fn image_version(&mut self, path: &Path) -> Result<String, String>;
}

#[derive(Deserialize)]
struct Release {
    action: u32,
    version: String,
    request_version: String,
    zip_package_url: String,
    md5: String,
    #[serde(deserialize_with = "decimal")]
    package_size: u64,
}
fn decimal<'de, D: serde::Deserializer<'de>>(d: D) -> Result<u64, D::Error> {
    let value = serde_json::Value::deserialize(d)?;
    value
        .as_u64()
        .or_else(|| value.as_str()?.parse().ok())
        .ok_or_else(|| serde::de::Error::custom("expected nonnegative decimal size"))
}
fn version_parts(text: &str) -> Option<Vec<u32>> {
    let parts: Vec<_> = text.split('.').collect();
    if !(2..=4).contains(&parts.len())
        || parts
            .iter()
            .any(|p| p.is_empty() || p.len() > 6 || !p.bytes().all(|b| b.is_ascii_digit()))
    {
        return None;
    }
    parts.into_iter().map(|p| p.parse().ok()).collect()
}
fn compare_versions(a: &str, b: &str) -> std::cmp::Ordering {
    let mut a = version_parts(a).unwrap();
    let mut b = version_parts(b).unwrap();
    a.resize(4, 0);
    b.resize(4, 0);
    a.cmp(&b)
}
fn hex_string(value: &str, size: usize) -> bool {
    value.len() == size && value.bytes().all(|b| b.is_ascii_hexdigit())
}
pub fn download_target(url: &str) -> Result<(&'static str, &str), String> {
    let path = url
        .strip_prefix("https://launcher-sign.hycdn.cn")
        .ok_or("unexpected official package origin")?;
    if !path.starts_with("/abYeZZ16BPluCFyT/launcher/1/1/endfield/")
        || path.contains('#')
        || !path.bytes().all(|b| (0x21..=0x7e).contains(&b))
        || path.contains('\\')
    {
        return Err("unexpected official package path".into());
    }
    Ok(("launcher-sign.hycdn.cn", path))
}
fn parse_release(bytes: &[u8], requested: &str) -> Result<Release, String> {
    if bytes.len() > 1_048_576 {
        return Err("launcher metadata exceeds 1 MiB".into());
    }
    // Do not include raw JSON/errors: the response contains temporary CDN signatures.
    let release: Release =
        serde_json::from_slice(bytes).map_err(|_| "invalid launcher metadata schema")?;
    if version_parts(&release.version).is_none()
        || release.request_version != requested
        || release.action > 1
    {
        return Err("unexpected launcher metadata version/action".into());
    }
    if release.action == 0 {
        if release.version != requested {
            return Err("no-update response changed the requested version".into());
        }
    } else {
        download_target(&release.zip_package_url)?;
        if !hex_string(&release.md5, 32) || !(1..=MAX_PACKAGE).contains(&release.package_size) {
            return Err("invalid official package size/MD5".into());
        }
        if compare_versions(&release.version, requested).is_lt() {
            return Err("refusing a launcher downgrade".into());
        }
    }
    Ok(release)
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct Receipt {
    schema: u32,
    directory: String,
    version: String,
    package_md5: String,
    games_sha256: String,
    games_size: u64,
    publisher: String,
}
pub struct ReadyParent {
    pub path: PathBuf,
    pub version: String,
    // Retained across CreateProcess and game lifetime, blocking image replacement on Windows.
    _image: File,
}
struct Cached {
    receipt: Receipt,
    ready: ReadyParent,
}
struct Candidate {
    ready: ReadyParent,
    package_md5: Option<String>,
    managed: bool,
}
impl From<Cached> for Candidate {
    fn from(cached: Cached) -> Self {
        Self {
            ready: cached.ready,
            package_md5: Some(cached.receipt.package_md5),
            managed: true,
        }
    }
}

fn path_key(path: &Path) -> String {
    let text = path.to_string_lossy().replace('/', "\\");
    text.strip_prefix(r"\\?\")
        .unwrap_or(&text)
        .trim_end_matches('\\')
        .to_lowercase()
}
fn within(path: &Path, root: &Path) -> bool {
    let path = path_key(path);
    let root = path_key(root);
    path == root || path.starts_with(&(root + "\\"))
}
pub fn validate_cache(root: &Path, forbidden: &[PathBuf]) -> Result<(), String> {
    if !root.is_absolute() || root.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err("--cache-dir must be an absolute path without ..".into());
    }
    for protected in forbidden {
        if within(root, protected) {
            return Err(
                "cache must be outside the game and launcher installation directories".into(),
            );
        }
    }
    reject_links(root)
}
fn reject_links(path: &Path) -> Result<(), String> {
    for part in path.ancestors() {
        match fs::symlink_metadata(part) {
            Ok(meta) => {
                #[cfg(windows)]
                let link = {
                    use std::os::windows::fs::MetadataExt;
                    meta.file_attributes() & 0x400 != 0
                };
                #[cfg(not(windows))]
                let link = meta.file_type().is_symlink();
                if link {
                    return Err("cache path contains a symlink or reparse point".into());
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(e) => return Err(format!("cannot inspect cache path: {e}")),
        }
    }
    Ok(())
}
pub(crate) fn pin_image(path: &Path) -> Result<File, String> {
    reject_links(path)?;
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(1); // FILE_SHARE_READ: neither writing nor deletion while pinned.
    }
    options
        .open(path)
        .map_err(|e| format!("cannot open cached Games.exe: {e}"))
}
fn unique() -> String {
    static COUNT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    format!(
        "{}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos(),
        COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    )
}
fn create_file(path: &Path) -> Result<File, String> {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| format!("cannot create cache file: {e}"))
}
fn sha256(reader: &mut impl Read) -> Result<String, String> {
    let mut hash = Sha256::new();
    let mut buffer = [0; 65536];
    loop {
        let n = reader
            .read(&mut buffer)
            .map_err(|e| format!("image hash read failed: {e}"))?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
    }
    Ok(format!("{:x}", hash.finalize()))
}
pub fn copy_exact(
    input: &mut impl Read,
    output: &mut dyn Write,
    expected: u64,
) -> Result<(), String> {
    let mut buffer = [0; 65536];
    let mut copied = 0u64;
    loop {
        let n = input
            .read(&mut buffer)
            .map_err(|e| format!("download read failed: {e}"))?;
        if n == 0 {
            break;
        }
        copied += n as u64;
        if copied > expected {
            return Err("transfer exceeds declared size".into());
        }
        output
            .write_all(&buffer[..n])
            .map_err(|e| format!("cache write failed: {e}"))?;
    }
    if copied != expected {
        return Err("transfer is shorter than declared size".into());
    }
    Ok(())
}

fn read_cache(root: &Path, backend: &mut impl Backend) -> Result<Option<Cached>, String> {
    let current = root.join("current.json");
    reject_links(&current)?;
    let mut file = match File::open(&current) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("cannot read cache receipt: {e}")),
    };
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(65537)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 65536 {
        return Err("cache receipt is oversized".into());
    }
    let receipt: Receipt = serde_json::from_slice(&bytes).map_err(|_| "invalid cache receipt")?;
    if receipt.schema != 1
        || version_parts(&receipt.version).is_none()
        || !hex_string(&receipt.package_md5, 32)
        || !hex_string(&receipt.games_sha256, 64)
        || !(1..=MAX_IMAGE).contains(&receipt.games_size)
        || receipt.publisher != PUBLISHER
        || receipt.directory.len() > 160
        || receipt.directory.is_empty()
        || receipt.directory == "."
        || receipt.directory == ".."
        || !receipt
            .directory
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'.')
    {
        return Err("invalid cache receipt fields".into());
    }
    let path = root
        .join("releases")
        .join(&receipt.directory)
        .join("Games.exe");
    let mut image = pin_image(&path)?;
    if image.metadata().map_err(|e| e.to_string())?.len() != receipt.games_size
        || sha256(&mut image)? != receipt.games_sha256
    {
        return Err("cached Games.exe size/SHA-256 mismatch".into());
    }
    if backend.verify(&path)? != PUBLISHER {
        return Err("cached Games.exe publisher mismatch".into());
    }
    if backend.image_version(&path)? != receipt.version {
        return Err("cached Games.exe file version mismatch".into());
    }
    let ready = ReadyParent {
        path,
        version: receipt.version.clone(),
        _image: image,
    };
    Ok(Some(Cached { receipt, ready }))
}

struct Stage(PathBuf);
impl Drop for Stage {
    fn drop(&mut self) {
        // Only exact files in this invocation's freshly created directory; no recursive deletion.
        for name in ["launcher.zip", "Games.exe", "receipt.json"] {
            let _ = fs::remove_file(self.0.join(name));
        }
        let _ = fs::remove_dir(&self.0);
    }
}
fn extract_games(package: &Path, destination: &Path) -> Result<u64, String> {
    let file = File::open(package).map_err(|e| e.to_string())?;
    let mut archive = zip::ZipArchive::new(file).map_err(|_| "invalid launcher ZIP")?;
    if archive.len() > 10000 {
        return Err("launcher ZIP contains too many entries".into());
    }
    let mut selected = None;
    for i in 0..archive.len() {
        let entry = archive
            .by_index(i)
            .map_err(|_| "cannot read ZIP directory")?;
        let name = entry.name().rsplit(['/', '\\']).next().unwrap_or_default();
        if name.eq_ignore_ascii_case("Games.exe") {
            if selected.is_some() || entry.is_dir() || entry.is_symlink() {
                return Err("ambiguous Games.exe ZIP entries".into());
            }
            selected = Some(i);
        }
    }
    let mut entry = archive
        .by_index(selected.ok_or("Games.exe missing from launcher ZIP")?)
        .map_err(|_| "cannot open Games.exe entry")?;
    let size = entry.size();
    if !(1..=MAX_IMAGE).contains(&size) {
        return Err("Games.exe ZIP entry exceeds size limit".into());
    }
    let mut output = create_file(destination)?;
    // The member path is never used as an output pathname. Reading to EOF checks ZIP CRC.
    copy_exact(&mut entry, &mut output, size)?;
    output.sync_all().map_err(|e| e.to_string())?;
    Ok(size)
}
fn install(
    root: &Path,
    release: Release,
    backend: &mut impl Backend,
) -> Result<ReadyParent, String> {
    let id = unique();
    let staging = root.join(format!(".pending-{id}"));
    fs::create_dir(&staging).map_err(|e| format!("cannot create staging directory: {e}"))?;
    let stage = Stage(staging);
    let package = stage.0.join("launcher.zip");
    crate::logging::info(
        format!(
            "正在下载官方启动器 {}（{} 字节），仅提取 Games.exe。",
            release.version, release.package_size
        ),
        format!(
            "Downloading official launcher {} ({} bytes); extracting only Games.exe.",
            release.version, release.package_size
        ),
    );
    {
        let mut file = create_file(&package)?;
        backend.download(&release.zip_package_url, &mut file, release.package_size)?;
        file.sync_all().map_err(|e| e.to_string())?;
    }
    if fs::metadata(&package).map_err(|e| e.to_string())?.len() != release.package_size {
        return Err("downloaded package size mismatch".into());
    }
    let mut hash = Md5::new();
    let mut input = pin_image(&package)?;
    let mut buffer = [0; 65536];
    loop {
        let n = input.read(&mut buffer).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
    }
    if format!("{:x}", hash.finalize()) != release.md5.to_ascii_lowercase() {
        return Err("official package MD5 mismatch; current cache was kept".into());
    }
    let image_path = stage.0.join("Games.exe");
    let size = extract_games(&package, &image_path)?;
    drop(input);
    let mut staged_image = pin_image(&image_path)?;
    let publisher = backend.verify(&image_path)?;
    if publisher != PUBLISHER {
        return Err("downloaded Games.exe publisher mismatch".into());
    }
    if backend.image_version(&image_path)? != release.version {
        return Err("downloaded Games.exe file version does not match release metadata".into());
    }
    let image_hash = sha256(&mut staged_image)?;
    let directory = format!(
        "{}-{}-{id}",
        release.version,
        release.md5.to_ascii_lowercase()
    );
    let receipt = Receipt {
        schema: 1,
        directory: directory.clone(),
        version: release.version.clone(),
        package_md5: release.md5.to_ascii_lowercase(),
        games_sha256: image_hash,
        games_size: size,
        publisher,
    };
    let serialized = serde_json::to_vec_pretty(&receipt).map_err(|e| e.to_string())?;
    {
        let mut file = create_file(&stage.0.join("receipt.json"))?;
        file.write_all(&serialized).map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
    }
    fs::remove_file(&package).map_err(|e| format!("cannot remove staged ZIP: {e}"))?;
    // Directory rename may require closing the staging pin on Windows. Re-pin
    // and compare the signed bytes before publishing the new receipt.
    drop(staged_image);
    let final_dir = root.join("releases").join(directory);
    fs::rename(&stage.0, &final_dir)
        .map_err(|e| format!("cannot commit release directory: {e}"))?;
    let path = final_dir.join("Games.exe");
    let mut image = pin_image(&path)?;
    if image.metadata().map_err(|e| e.to_string())?.len() != receipt.games_size
        || sha256(&mut image)? != receipt.games_sha256
    {
        return Err("Games.exe changed before cache commit; current cache was kept".into());
    }
    let current_tmp = root.join(format!("current-{id}.tmp"));
    let committed = (|| {
        let mut file = create_file(&current_tmp)?;
        file.write_all(&serialized).map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
        drop(file);
        fs::rename(&current_tmp, root.join("current.json"))
            .map_err(|e| format!("cannot switch current cache: {e}"))
    })();
    if committed.is_err() {
        let _ = fs::remove_file(current_tmp);
    }
    committed?;
    crate::logging::info(
        format!("官方 Games.exe {} 已通过校验并写入缓存。", release.version),
        format!(
            "Official Games.exe {} verified and cached.",
            release.version
        ),
    );
    Ok(ReadyParent {
        path,
        version: release.version,
        _image: image,
    })
}

pub fn ensure(
    root: &Path,
    forbidden: &[PathBuf],
    locals: &[PathBuf],
    offline: bool,
    backend: &mut impl Backend,
) -> Result<ReadyParent, String> {
    validate_cache(root, forbidden)?;
    fs::create_dir_all(root.join("releases"))
        .map_err(|e| format!("cannot create managed cache: {e}"))?;
    reject_links(&root.join("releases"))?;
    let lock_path = root.join("update.lock");
    reject_links(&lock_path)?;
    let mut open = OpenOptions::new();
    open.read(true).write(true).create(true).truncate(false);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        open.share_mode(0);
    }
    let _lock = open
        .open(lock_path)
        .map_err(|e| format!("cannot lock cache; another updater may be running: {e}"))?;
    let mut candidate: Option<Candidate> = match read_cache(root, backend) {
        Ok(value) => value.map(Into::into),
        Err(e) => {
            crate::logging::error(
                format!("缓存父进程映像校验失败：{e}"),
                format!("Cached parent rejected: {e}"),
            );
            None
        }
    };
    for path in locals {
        if !path.is_file() {
            continue;
        }
        let inspected: Result<ReadyParent, String> = (|| {
            let image = pin_image(path)?;
            if image.metadata().map_err(|e| e.to_string())?.len() > MAX_IMAGE {
                return Err("local parent exceeds size limit".into());
            }
            if backend.verify(path)? != PUBLISHER {
                return Err("local parent publisher mismatch".into());
            }
            let version = backend.image_version(path)?;
            if version_parts(&version).is_none() {
                return Err("local parent version is unsupported".into());
            }
            Ok(ReadyParent {
                path: path.clone(),
                version,
                _image: image,
            })
        })();
        match inspected {
            Ok(ready) => {
                if candidate.as_ref().is_none_or(|previous| {
                    let order = compare_versions(&ready.version, &previous.ready.version);
                    order.is_gt() || (order.is_eq() && previous.managed)
                }) {
                    candidate = Some(Candidate {
                        ready,
                        package_md5: None,
                        managed: false,
                    });
                }
            }
            Err(error) => crate::logging::error(
                format!("已忽略本地 Games.exe（{}）：{error}", path.display()),
                format!("Local Games.exe ignored ({}): {error}", path.display()),
            ),
        }
    }
    if offline {
        crate::logging::info(
            "离线模式：不检查官方最新版本。",
            "Offline mode: official latest version is not checked.",
        );
        return candidate
            .map(|c| c.ready)
            .ok_or_else(|| "--offline needs a verified cached or local Games.exe".into());
    }
    let requested = candidate
        .as_ref()
        .map(|c| c.ready.version.as_str())
        .unwrap_or("0.0.0");
    crate::logging::info(
        format!("正在检查官方启动器更新，当前可用版本：{requested}。"),
        format!("Checking official launcher updates (available version: {requested})."),
    );
    let bytes = match backend.metadata(requested) {
        Ok(bytes) => bytes,
        Err(error) => {
            let fallback = match candidate {
                Some(c) if c.managed => Some(c.ready),
                _ => read_cache(root, backend).ok().flatten().map(|c| c.ready),
            };
            if let Some(ready) = fallback {
                crate::logging::error(
                    format!(
                        "更新检查失败：{error}。使用已校验的缓存版本 {}，尚未确认是否为最新版。",
                        ready.version
                    ),
                    format!(
                        "Update check failed: {error}. Using verified cached {} (latest version unconfirmed).",
                        ready.version
                    ),
                );
                return Ok(ready);
            }
            return Err(format!(
                "cannot obtain official Games.exe on first use: {error}"
            ));
        }
    };
    let release = parse_release(&bytes, requested)?;
    if let Some(candidate) = candidate
        && (release.action == 0
            || (release.version == candidate.ready.version
                && (!candidate.managed
                    || candidate
                        .package_md5
                        .as_ref()
                        .is_some_and(|m| release.md5.eq_ignore_ascii_case(m)))))
    {
        crate::logging::info(
            format!(
                "使用已校验的{} Games.exe {}：{}，无需下载。",
                if candidate.managed {
                    "缓存"
                } else {
                    "本地"
                },
                candidate.ready.version,
                candidate.ready.path.display()
            ),
            format!(
                "Using verified {} Games.exe {}: {}; no download needed.",
                if candidate.managed { "cached" } else { "local" },
                candidate.ready.version,
                candidate.ready.path.display()
            ),
        );
        return Ok(candidate.ready);
    }
    if release.action != 1 {
        return Err("official service did not provide a package for first use".into());
    }
    install(root, release, backend)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    struct Workspace(PathBuf);
    impl Workspace {
        fn new() -> Self {
            let path = std::env::current_dir()
                .unwrap()
                .join("target")
                .join(format!("acquisition-test-{}", unique()));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
        fn cache(&self) -> PathBuf {
            self.0.join("cache")
        }
    }
    impl Drop for Workspace {
        fn drop(&mut self) {
            // Test-created workspace only, with an explicit containment check.
            let root = std::env::current_dir().unwrap().join("target");
            assert!(self.0.starts_with(root));
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn archive(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, content) in entries {
            zip.start_file(
                *name,
                zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Deflated),
            )
            .unwrap();
            zip.write_all(content).unwrap();
        }
        zip.finish().unwrap().into_inner()
    }
    struct Mock {
        version: String,
        package: Vec<u8>,
        downloads: usize,
        queries: Vec<String>,
        offline: bool,
        bad_md5: bool,
        bad_signature: bool,
    }
    impl Mock {
        fn new(version: &str) -> Self {
            let image = format!("{version}|signed-fixture");
            Self {
                version: version.into(),
                package: archive(&[
                    ("Games.exe", image.as_bytes()),
                    ("unused.dll", b"do not extract"),
                ]),
                downloads: 0,
                queries: vec![],
                offline: false,
                bad_md5: false,
                bad_signature: false,
            }
        }
    }
    impl Backend for Mock {
        fn metadata(&mut self, version: &str) -> Result<Vec<u8>, String> {
            self.queries.push(version.into());
            if self.offline {
                return Err("test connection unavailable".into());
            }
            let update = self.version != version;
            Ok(serde_json::to_vec(&serde_json::json!({
                "action": if update {1} else {0}, "version": self.version, "request_version": version,
                "zip_package_url": if update {"https://launcher-sign.hycdn.cn/abYeZZ16BPluCFyT/launcher/1/1/endfield/release.zip?auth_key=not-persisted"} else {""},
                "md5": if update {if self.bad_md5 {"0".repeat(32)} else {format!("{:x}", Md5::digest(&self.package))}} else {String::new()},
                "package_size": if update {self.package.len().to_string()} else {"0".into()}
            })).unwrap())
        }
        fn download(&mut self, _: &str, out: &mut dyn Write, size: u64) -> Result<(), String> {
            self.downloads += 1;
            copy_exact(&mut Cursor::new(&self.package), out, size)
        }
        fn verify(&mut self, path: &Path) -> Result<String, String> {
            if self.bad_signature
                || !fs::read(path)
                    .map_err(|e| e.to_string())?
                    .ends_with(b"|signed-fixture")
            {
                return Err("test signature rejected".into());
            }
            Ok(PUBLISHER.into())
        }
        fn image_version(&mut self, path: &Path) -> Result<String, String> {
            Ok(fs::read_to_string(path)
                .map_err(|e| e.to_string())?
                .split('|')
                .next()
                .unwrap()
                .into())
        }
    }
    fn prepare(work: &Workspace, mock: &mut Mock) -> Result<ReadyParent, String> {
        ensure(&work.cache(), &[], &[], false, mock)
    }

    #[test]
    fn first_use_then_no_update_downloads_only_once() {
        let work = Workspace::new();
        let mut mock = Mock::new("1.6.0");
        let first = prepare(&work, &mut mock).unwrap();
        assert_eq!(first.version, "1.6.0");
        assert!(!first.path.parent().unwrap().join("unused.dll").exists());
        assert!(!first.path.parent().unwrap().join("launcher.zip").exists());
        let receipt = fs::read_to_string(work.cache().join("current.json")).unwrap();
        assert!(!receipt.contains("auth_key"));
        let second = prepare(&work, &mut mock).unwrap();
        assert_eq!(first.path, second.path);
        assert_eq!(mock.queries, ["0.0.0", "1.6.0"]);
        assert_eq!(mock.downloads, 1);
    }
    #[test]
    fn update_commits_new_release_without_touching_pinned_old_image() {
        let work = Workspace::new();
        let old = prepare(&work, &mut Mock::new("1.6.0")).unwrap();
        let before = fs::read(&old.path).unwrap();
        let new = prepare(&work, &mut Mock::new("1.7.0")).unwrap();
        assert_eq!(new.version, "1.7.0");
        assert_ne!(old.path, new.path);
        assert_eq!(fs::read(&old.path).unwrap(), before);
        assert_eq!(
            read_cache(&work.cache(), &mut Mock::new("1.7.0"))
                .unwrap()
                .unwrap()
                .ready
                .path,
            new.path
        );
    }
    #[test]
    fn bad_hash_or_signature_keeps_previous_current_receipt() {
        for failure in 0..3 {
            let work = Workspace::new();
            let old = prepare(&work, &mut Mock::new("1.6.0")).unwrap();
            let before = fs::read(work.cache().join("current.json")).unwrap();
            let mut bad = Mock::new("1.7.0");
            if failure == 1 {
                bad.package = archive(&[("Games.exe", b"1.7.0|unsigned-fixture")]);
            } else if failure == 2 {
                bad.package = archive(&[("Games.exe", b"1.6.0|signed-fixture")]);
            } else {
                bad.bad_md5 = true;
            }
            assert!(prepare(&work, &mut bad).is_err());
            assert_eq!(fs::read(work.cache().join("current.json")).unwrap(), before);
            assert!(old.path.exists());
            assert!(!fs::read_dir(work.cache()).unwrap().any(|e| {
                e.unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".pending-")
            }));
        }
    }
    #[test]
    fn offline_fallback_requires_valid_existing_cache() {
        let work = Workspace::new();
        let mut offline = Mock::new("1.6.0");
        offline.offline = true;
        assert!(prepare(&work, &mut offline).is_err());
        let old = prepare(&work, &mut Mock::new("1.6.0")).unwrap();
        let fallback = prepare(&work, &mut offline).unwrap();
        assert_eq!(fallback.path, old.path);
        let queries = offline.queries.len();
        assert!(ensure(&work.cache(), &[], &[], true, &mut offline).is_ok());
        assert_eq!(offline.queries.len(), queries);
    }
    #[test]
    fn corrupt_cached_image_is_not_reused() {
        let work = Workspace::new();
        let mut mock = Mock::new("1.6.0");
        let old = prepare(&work, &mut mock).unwrap();
        let old_path = old.path.clone();
        drop(old);
        fs::write(&old_path, b"corrupt").unwrap();
        let repaired = prepare(&work, &mut mock).unwrap();
        assert_ne!(old_path, repaired.path);
        assert_eq!(mock.downloads, 2);
        assert_eq!(mock.queries.last().unwrap(), "0.0.0");
    }
    #[test]
    fn matching_local_parent_is_used_in_place_and_older_one_triggers_download() {
        let work = Workspace::new();
        let local = work.0.join("Games.exe");
        fs::write(&local, b"1.6.0|signed-fixture").unwrap();
        let mut mock = Mock::new("1.6.0");
        let found = ensure(
            &work.cache(),
            &[],
            std::slice::from_ref(&local),
            false,
            &mut mock,
        )
        .unwrap();
        assert_eq!(found.path, local);
        assert_eq!(mock.downloads, 0);
        assert!(!work.cache().join("current.json").exists());
        let mut newer = Mock::new("1.7.0");
        let updated = ensure(
            &work.cache(),
            &[],
            std::slice::from_ref(&local),
            false,
            &mut newer,
        )
        .unwrap();
        assert_ne!(updated.path, local);
        assert_eq!(newer.downloads, 1);
        assert_eq!(fs::read(&local).unwrap(), b"1.6.0|signed-fixture");
    }
    #[test]
    fn unsigned_local_parent_is_rejected() {
        let work = Workspace::new();
        let local = work.0.join("Games.exe");
        fs::write(&local, b"1.6.0|unsigned").unwrap();
        let mut mock = Mock::new("1.6.0");
        let ready = ensure(
            &work.cache(),
            &[],
            std::slice::from_ref(&local),
            false,
            &mut mock,
        )
        .unwrap();
        assert_ne!(ready.path, local);
        assert_eq!(mock.downloads, 1);
    }
    #[test]
    fn archive_paths_cannot_escape_and_duplicates_are_rejected() {
        let work = Workspace::new();
        let package = work.0.join("test.zip");
        let out = work.0.join("Games.exe");
        fs::write(&package, archive(&[("../../Games.exe", b"bytes")])).unwrap();
        assert_eq!(extract_games(&package, &out).unwrap(), 5);
        assert_eq!(fs::read(&out).unwrap(), b"bytes");
        fs::remove_file(out).unwrap();
        fs::write(
            &package,
            archive(&[("Games.exe", b"one"), ("other/Games.exe", b"two")]),
        )
        .unwrap();
        assert!(extract_games(&package, &work.0.join("Games.exe")).is_err());
    }
    #[test]
    fn transfer_and_metadata_boundaries_are_enforced() {
        for (bytes, expected, success) in [(8, 8, true), (7, 8, false), (9, 8, false)] {
            let mut out = Vec::new();
            assert_eq!(
                copy_exact(&mut Cursor::new(vec![1; bytes]), &mut out, expected).is_ok(),
                success
            );
            assert!(out.len() as u64 <= expected);
        }
        for url in [
            "http://launcher-sign.hycdn.cn/x",
            "https://launcher-sign.hycdn.cn.evil/x",
            "https://user@launcher-sign.hycdn.cn/x",
            "https://launcher-sign.hycdn.cn/elsewhere.zip",
        ] {
            assert!(download_target(url).is_err());
        }
        let mut mock = Mock::new("1.6.0");
        let bytes = mock.metadata("0.0.0").unwrap();
        assert!(parse_release(&bytes, "1.5.0").is_err());
        mock.version = "../escape".into();
        assert!(parse_release(&mock.metadata("0.0.0").unwrap(), "0.0.0").is_err());
    }
    #[test]
    fn rejects_cache_inside_installation_even_with_different_ascii_case() {
        let work = Workspace::new();
        assert!(validate_cache(&work.0.join("game/cache"), &[work.0.join("GAME")]).is_err());
    }
}
