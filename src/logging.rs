use std::fmt::Display;
use std::io::{self, Write};

const SEPARATOR: &str = "================";

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

fn entry(chinese: impl Display, english: impl Display) -> String {
    format!(
        "{SEPARATOR}\n{}\n{}\n{SEPARATOR}\n",
        single_line(chinese),
        single_line(english)
    )
}

pub fn info(chinese: impl Display, english: impl Display) {
    let mut output = io::stdout().lock();
    let _ = output.write_all(entry(chinese, english).as_bytes());
    let _ = output.flush();
}

pub fn error(chinese: impl Display, english: impl Display) {
    let mut output = io::stderr().lock();
    let _ = output.write_all(entry(chinese, english).as_bytes());
    let _ = output.flush();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_have_two_message_lines_with_chinese_first_and_exact_borders() {
        assert_eq!(
            entry("游戏进程已创建：123", "Game process created: 123"),
            "================\n游戏进程已创建：123\nGame process created: 123\n================\n"
        );
        let record = entry("路径含\n换行\r\n", "detail\tvalue\x1b[2J\u{2028}");
        assert_eq!(record.lines().count(), 4);
        assert!(record.contains("路径含\\n换行\\r\\n"));
        assert!(record.contains("detail\\tvalue\\u{1b}[2J\\u{2028}"));
    }
}
