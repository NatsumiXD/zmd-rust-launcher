//! Display language is separate from the game's --lang launch argument.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, Ordering};

const SETTINGS_FILE: &str = "launcher-settings.json";
static CURRENT: AtomicU8 = AtomicU8::new(0);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Language {
    #[default]
    #[serde(rename = "en", alias = "en-us")]
    English,
    #[serde(rename = "zh-cn", alias = "cn", alias = "zh")]
    Chinese,
}

impl Language {
    pub(crate) fn toggled(self) -> Self {
        match self {
            Self::English => Self::Chinese,
            Self::Chinese => Self::English,
        }
    }

    pub(crate) fn text<'a>(self, chinese: &'a str, english: &'a str) -> &'a str {
        match self {
            Self::Chinese => chinese,
            Self::English => english,
        }
    }

    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().as_str() {
            "cn" | "zh" | "zh-cn" => Some(Self::Chinese),
            "en" | "en-us" => Some(Self::English),
            _ => None,
        }
    }
}

pub(crate) fn current() -> Language {
    if CURRENT.load(Ordering::Relaxed) == 1 {
        Language::Chinese
    } else {
        Language::English
    }
}

fn select(language: Language) {
    CURRENT.store(u8::from(language == Language::Chinese), Ordering::Relaxed);
}

pub(crate) fn text<'a>(chinese: &'a str, english: &'a str) -> &'a str {
    current().text(chinese, english)
}

#[derive(Default, Serialize, Deserialize)]
struct Settings {
    #[serde(default)]
    language: Language,
    // Leave future/unrelated settings intact when changing the language.
    #[serde(flatten)]
    extra: BTreeMap<String, serde_json::Value>,
}

fn path() -> io::Result<PathBuf> {
    let executable = std::env::current_exe()?;
    Ok(executable
        .parent()
        .ok_or_else(|| io::Error::other("launcher has no directory"))?
        .join(SETTINGS_FILE))
}

fn read(path: &Path) -> io::Result<Settings> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Settings::default()),
        Err(error) => return Err(error),
    };
    let mut bytes = Vec::new();
    file.take(65_537).read_to_end(&mut bytes)?;
    if bytes.len() > 65_536 {
        return Err(io::Error::other("launcher settings exceed 64 KiB"));
    }
    serde_json::from_slice(&bytes).map_err(io::Error::other)
}

fn load(path: &Path) -> io::Result<()> {
    select(read(path)?.language);
    Ok(())
}

pub(crate) fn initialize() {
    match path().and_then(|path| load(&path)) {
        Ok(()) => {}
        Err(error) => {
            select(Language::English);
            crate::logging::error(
                format!("无法读取语言设置，暂用英文：{error}"),
                format!("Cannot read language settings; using English for this session: {error}"),
            );
        }
    }
}

fn save(path: &Path, language: Language, protected: &[PathBuf]) -> Result<(), String> {
    crate::acquisition::validate_cache(path, protected)?;
    let mut settings = read(path).map_err(|error| error.to_string())?;
    settings.language = language;
    let bytes = serde_json::to_vec_pretty(&settings).map_err(|error| error.to_string())?;
    if bytes.len() > 65_536 {
        return Err("launcher settings exceed 64 KiB".into());
    }
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_nanos();
    let temporary = path.with_file_name(format!(
        "{SETTINGS_FILE}.{}-{stamp}.tmp",
        std::process::id()
    ));
    let result = (|| -> io::Result<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);
        #[cfg(windows)]
        {
            use std::os::windows::ffi::OsStrExt;
            use windows_sys::Win32::Storage::FileSystem::{
                MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
            };
            let from: Vec<u16> = temporary.as_os_str().encode_wide().chain(Some(0)).collect();
            let to: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
            // SAFETY: both paths are validated, live, NUL-terminated strings.
            if unsafe {
                MoveFileExW(
                    from.as_ptr(),
                    to.as_ptr(),
                    MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
                )
            } == 0
            {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        }
        #[cfg(not(windows))]
        fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result.map_err(|error| error.to_string())
}

pub(crate) fn change(requested: Option<Language>, game: &Path) -> Result<(), String> {
    let path = path().map_err(|error| error.to_string())?;
    let protected = [
        PathBuf::from(r"D:\yj\Hypergryph Launcher"),
        game.parent().ok_or("game has no directory")?.to_path_buf(),
    ];
    change_at(requested, &path, &protected)
}

fn change_at(
    requested: Option<Language>,
    path: &Path,
    protected: &[PathBuf],
) -> Result<(), String> {
    let language = requested.unwrap_or_else(|| current().toggled());
    save(path, language, protected)?;
    // Only change the session after the setting has been saved successfully.
    select(language);
    crate::logging::info(
        format!(
            "默认显示语言已设为{}，设置已保存：{}",
            language.text("中文", "英文"),
            path.display()
        ),
        format!(
            "Default display language set to {}; saved to {}",
            language.text("Chinese", "English"),
            path.display()
        ),
    );
    Ok(())
}

/// Called only inside the isolated console-test subprocess; never touches the
/// user's real preferences or changes language in the parallel unit-test host.
#[cfg(test)]
pub(crate) fn probe_saved_language(check_console: impl FnOnce()) {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("artifacts")
        .join(format!("language-console-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let path = root.join(SETTINGS_FILE);
    save(&path, Language::English, &[]).unwrap();
    load(&path).unwrap();
    change_at(None, &path, &[]).unwrap();
    assert_eq!(current(), Language::Chinese);
    select(Language::English); // Simulate a fresh process before loading preferences.
    load(&path).unwrap();
    assert_eq!(current(), Language::Chinese);
    check_console();
    crate::logging::print_chinese_history().unwrap();
    assert_eq!(
        current(),
        Language::Chinese,
        "cn must not change the default"
    );
    assert!(change_at(Some(Language::English), &path, std::slice::from_ref(&root)).is_err());
    assert_eq!(
        current(),
        Language::Chinese,
        "failed saves must not change language"
    );
    change_at(None, &path, &[]).unwrap();
    assert_eq!(current(), Language::English);
    assert_eq!(read(&path).unwrap().language, Language::English);
    fs::write(
        root.join("verification.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "toggle_saved_and_reloaded": true,
            "cn_preserves_default_language": true,
            "failed_save_preserves_current_language": true,
            "native_chinese_hint_and_cursor_verified": true,
            "user_settings_modified": false
        }))
        .unwrap(),
    )
    .unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip_preserves_other_fields_and_protects_installations() {
        let root = std::env::current_dir()
            .unwrap()
            .join("artifacts")
            .join(format!("language-test-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let path = root.join(SETTINGS_FILE);
        fs::write(&path, br#"{"language":"en","future":42}"#).unwrap();
        save(&path, Language::Chinese, &[]).unwrap();
        let settings = read(&path).unwrap();
        assert_eq!(settings.language, Language::Chinese);
        assert_eq!(settings.extra["future"], 42);
        save(&path, Language::English, &[]).unwrap();
        assert_eq!(read(&path).unwrap().language, Language::English);
        let before = fs::read(&path).unwrap();
        assert!(save(&path, Language::Chinese, std::slice::from_ref(&root)).is_err());
        assert_eq!(fs::read(&path).unwrap(), before);
        fs::write(&path, b"broken json").unwrap();
        assert!(save(&path, Language::Chinese, &[]).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"broken json");
        assert_eq!(
            read(&root.join("missing.json")).unwrap().language,
            Language::English
        );
    }
}
