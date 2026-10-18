use std::collections::VecDeque;
use std::fmt::Display;
use std::io::{self, Write};
use std::sync::Mutex;

const MAX_RECORDS: usize = 4096;
const MAX_BYTES: usize = 4 * 1024 * 1024;

struct Record {
    id: u64,
    error: bool,
    chinese: String,
    english: String,
}

impl Record {
    fn line(&self, chinese: bool) -> String {
        format!(
            "[{:04} {}] {}\n",
            self.id,
            if self.error { "ERROR" } else { "INFO" },
            if chinese {
                &self.chinese
            } else {
                &self.english
            }
        )
    }
}

struct History {
    records: VecDeque<Record>,
    next_id: u64,
    bytes: usize,
    dropped: usize,
}

impl History {
    const fn new() -> Self {
        Self {
            records: VecDeque::new(),
            next_id: 1,
            bytes: 0,
            dropped: 0,
        }
    }

    fn push(&mut self, chinese: impl Display, english: impl Display, error: bool) -> String {
        let record = Record {
            id: self.next_id,
            error,
            chinese: single_line(chinese),
            english: single_line(english),
        };
        self.next_id = self.next_id.saturating_add(1);
        let output = record.line(false);
        self.bytes += record.chinese.len() + record.english.len();
        self.records.push_back(record);
        while self.records.len() > MAX_RECORDS || self.bytes > MAX_BYTES {
            if let Some(old) = self.records.pop_front() {
                self.bytes -= old.chinese.len() + old.english.len();
                self.dropped += 1;
            }
        }
        output
    }

    fn chinese(&self) -> String {
        let mut output = format!("--- 历史日志中文翻译（{} 条）---\n", self.records.len());
        if self.dropped > 0 {
            output.push_str(&format!(
                "更早的 {} 条记录已超出会话历史上限，以下为保留的记录。\n",
                self.dropped
            ));
        }
        for record in &self.records {
            output.push_str(&record.line(true));
        }
        output.push_str("--- 翻译结束；后续日志仍使用英文 ---\n");
        output
    }
}

static HISTORY: Mutex<History> = Mutex::new(History::new());
static JOURNAL: Mutex<Option<std::fs::File>> = Mutex::new(None);

#[derive(serde::Serialize, serde::Deserialize)]
pub(crate) struct JournalRecord {
    pub chinese: String,
    pub english: String,
    pub error: bool,
}
pub(crate) fn attach_journal(path: &std::path::Path) -> io::Result<()> {
    *JOURNAL.lock().unwrap_or_else(|value| value.into_inner()) = Some(
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)?,
    );
    Ok(())
}
impl JournalRecord {
    pub(crate) fn replay(self) {
        emit(self.chinese, self.english, self.error);
    }
}

fn single_line(value: impl Display) -> String {
    let mut output = String::new();
    for character in value.to_string().chars() {
        if character.is_control() || matches!(character, '\u{2028}' | '\u{2029}') {
            output.extend(character.escape_default());
        } else {
            output.push(character);
        }
    }
    output
}

fn emit(chinese: impl Display, english: impl Display, error: bool) {
    let mut history = HISTORY
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let record = JournalRecord {
        chinese: chinese.to_string(),
        english: english.to_string(),
        error,
    };
    let line = history.push(&record.chinese, &record.english, error);
    if let Some(file) = JOURNAL
        .lock()
        .unwrap_or_else(|value| value.into_inner())
        .as_mut()
    {
        if let Ok(mut bytes) = serde_json::to_vec(&record) {
            bytes.push(b'\n');
            let _ = file.write_all(&bytes).and_then(|_| file.flush());
        }
        return;
    }
    if error {
        let mut output = io::stderr().lock();
        let _ = output.write_all(line.as_bytes());
        let _ = output.flush();
    } else {
        let mut output = io::stdout().lock();
        let _ = output.write_all(line.as_bytes());
        let _ = output.flush();
    }
}

pub fn info(chinese: impl Display, english: impl Display) {
    emit(chinese, english, false);
}
pub fn error(chinese: impl Display, english: impl Display) {
    emit(chinese, english, true);
}

#[cfg(windows)]
pub fn print_chinese_history() -> io::Result<()> {
    let snapshot = HISTORY
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .chinese();
    let mut output = io::stdout().lock();
    output.write_all(snapshot.as_bytes())?;
    output.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn english_logs_are_single_lines_and_cn_replays_history_without_changing_output_language() {
        let mut history = History::new();
        assert_eq!(
            history.push("游戏已启动", "Game started", false),
            "[0001 INFO] Game started\n"
        );
        assert_eq!(
            history
                .push("路径含\n换行", "detail\tvalue\x1b[2J\u{2028}", true)
                .lines()
                .count(),
            1
        );
        let before = history.records.len();
        let translated = history.chinese();
        assert!(translated.contains("[0001 INFO] 游戏已启动"));
        assert!(translated.contains("路径含\\n换行"));
        assert_eq!(history.records.len(), before, "replay must not log itself");
        assert_eq!(history.chinese(), translated);
        assert_eq!(
            history.push("下一条", "Next entry", false),
            "[0003 INFO] Next entry\n"
        );
    }

    #[test]
    fn bounded_history_reports_omissions_and_keeps_recent_entries() {
        let mut history = History::new();
        for _ in 0..MAX_RECORDS + 2 {
            history.push("中文", "English", false);
        }
        assert_eq!(history.records.len(), MAX_RECORDS);
        assert_eq!(history.dropped, 2);
        assert_eq!(history.records.front().unwrap().id, 3);
        assert!(history.chinese().contains("更早的 2 条"));
        let mut history = History::new();
        history.push("x".repeat(MAX_BYTES + 1), "Oversized entry", false);
        assert!(history.records.is_empty());
        assert_eq!(history.dropped, 1);
    }
}
