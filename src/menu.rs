use crate::language::{set_chinese, text};
use crate::{Action, Options};
use std::io::{self, Write};
use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::System::Console::*;

#[link(name = "kernel32")]
unsafe extern "system" {
    fn WaitForSingleObject(handle: HANDLE, milliseconds: u32) -> u32;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Key {
    Character(char),
    Backspace,
    Enter,
    Tab,
    StopGame,
    Exit,
    Other,
}

fn decode_key(key: KEY_EVENT_RECORD) -> Option<Key> {
    if key.bKeyDown == 0 {
        return None;
    }
    // SAFETY: a KEY_EVENT_RECORD carries a Unicode character in ReadConsoleInputW.
    let character = unsafe { key.uChar.UnicodeChar };
    Some(match (key.wVirtualKeyCode, character) {
        (_, 26) => Key::StopGame,
        (0x5a, _) if key.dwControlKeyState & (LEFT_CTRL_PRESSED | RIGHT_CTRL_PRESSED) != 0 => {
            Key::StopGame
        }
        (_, 3) => Key::Exit,
        (0x43, _) if key.dwControlKeyState & (LEFT_CTRL_PRESSED | RIGHT_CTRL_PRESSED) != 0 => {
            Key::Exit
        }
        (0x1b, _) => Key::Exit,
        (0x08, _) => Key::Backspace,
        (0x0d, _) => Key::Enter,
        (0x09, _) => Key::Tab,
        (_, character @ 0x20..=0x7e) => Key::Character(char::from_u32(character as u32).unwrap()),
        _ => Key::Other,
    })
}

#[derive(Default)]
struct MenuState {
    input: String,
    chinese: bool,
}

impl MenuState {
    fn handle(&mut self, key: Key) -> Result<Option<usize>, ()> {
        match key {
            Key::Character(character) if self.input.len() < 32 => self.input.push(character),
            Key::Character(_) => {}
            Key::Backspace => {
                self.input.pop();
            }
            Key::Tab => self.chinese = !self.chinese,
            Key::Enter => {
                let input = std::mem::take(&mut self.input);
                return match input.trim() {
                    "1" => Ok(Some(0)),
                    "2" => Ok(Some(1)),
                    "3" => Ok(Some(2)),
                    "0" | "q" | "Q" => Ok(Some(3)),
                    _ => Err(()),
                };
            }
            Key::Exit => return Ok(Some(3)),
            Key::Other => {}
            Key::StopGame => {}
        }
        Ok(None)
    }
}

// Scoped input mode: Ctrl+Z is read as a key rather than console EOF. The guard
// restores the caller's mode before normal acquisition and process creation.
struct Console {
    input: HANDLE,
    input_mode: u32,
}

impl Console {
    fn open() -> Result<Self, String> {
        // SAFETY: querying standard handles and console modes, with writable outputs.
        let input = unsafe { GetStdHandle(STD_INPUT_HANDLE) };
        let mut input_mode = 0;
        if unsafe { GetConsoleMode(input, &mut input_mode) } == 0 {
            return Err("The menu needs a Windows console; use --launch or --update-only for redirected input.".into());
        }
        let mode = (input_mode | ENABLE_EXTENDED_FLAGS)
            & !(ENABLE_LINE_INPUT
                | ENABLE_ECHO_INPUT
                | ENABLE_PROCESSED_INPUT
                | ENABLE_QUICK_EDIT_MODE
                | ENABLE_VIRTUAL_TERMINAL_INPUT);
        // SAFETY: the live console input handle was validated above.
        if unsafe { SetConsoleMode(input, mode) } == 0 {
            return Err(format!(
                "cannot configure menu input: {}",
                io::Error::last_os_error()
            ));
        }
        // Ignore keys typed while an earlier operation was still running.
        unsafe { FlushConsoleInputBuffer(input) };
        Ok(Self { input, input_mode })
    }

    fn poll_key(&self) -> Result<Option<Key>, String> {
        loop {
            // SAFETY: live console input handle. Timeout avoids a busy polling loop.
            match unsafe { WaitForSingleObject(self.input, 100) } {
                258 => return Ok(None),
                0 => {}
                _ => {
                    return Err(format!(
                        "menu input wait failed: {}",
                        io::Error::last_os_error()
                    ));
                }
            }
            let mut record = INPUT_RECORD::default();
            let mut count = 0;
            // SAFETY: one writable record and count, live input handle.
            if unsafe { ReadConsoleInputW(self.input, &mut record, 1, &mut count) } == 0 {
                return Err(format!("menu input failed: {}", io::Error::last_os_error()));
            }
            if count == 1 && record.EventType == KEY_EVENT as u16 {
                // SAFETY: EventType above identifies the active union member.
                if let Some(key) = decode_key(unsafe { record.Event.KeyEvent }) {
                    return Ok(Some(key));
                }
            }
        }
    }
}

#[cfg(test)]
pub(super) fn probe_console_keys() {
    // The caller creates an isolated, hidden console for this test host.
    let console = Console::open().unwrap();
    let mut state = MenuState::default();
    for (virtual_key, character, expected) in [
        (9, 9, Key::Tab),
        (0x32, 0x32, Key::Character('2')),
        (0x0d, 13, Key::Enter),
    ] {
        let record = INPUT_RECORD {
            EventType: KEY_EVENT as u16,
            Event: INPUT_RECORD_0 {
                KeyEvent: KEY_EVENT_RECORD {
                    bKeyDown: 1,
                    wRepeatCount: 1,
                    wVirtualKeyCode: virtual_key,
                    uChar: KEY_EVENT_RECORD_0 {
                        UnicodeChar: character,
                    },
                    ..Default::default()
                },
            },
        };
        let mut written = 0;
        // SAFETY: live console and one initialized key-down record.
        assert_ne!(
            unsafe { WriteConsoleInputW(console.input, &record, 1, &mut written) },
            0
        );
        assert_eq!(written, 1);
        let key = loop {
            if let Some(key) = console.poll_key().unwrap() {
                break key;
            }
        };
        assert_eq!(key, expected);
        let choice = state.handle(key);
        if key == Key::Enter {
            assert_eq!(choice, Ok(Some(1)));
        } else {
            assert_eq!(choice, Ok(None));
        }
    }
    assert!(state.chinese);
    set_chinese(true);
    // Save a marker at its actual console position before rendering the menu.
    let output = unsafe { GetStdHandle(STD_OUTPUT_HANDLE) };
    let mut screen = CONSOLE_SCREEN_BUFFER_INFO::default();
    io::stdout().flush().unwrap();
    assert_ne!(
        unsafe { GetConsoleScreenBufferInfo(output, &mut screen) },
        0
    );
    let marker = "startup-log-must-remain";
    println!("{marker}");
    io::stdout().flush().unwrap();
    render(&state, &Options::default(), None).unwrap();
    let mut buffer = vec![0u16; marker.len()];
    let mut read = 0;
    assert_ne!(
        unsafe {
            ReadConsoleOutputCharacterW(
                output,
                buffer.as_mut_ptr(),
                buffer.len() as u32,
                screen.dwCursorPosition,
                &mut read,
            )
        },
        0
    );
    assert_eq!(
        String::from_utf16(&buffer[..read as usize]).unwrap(),
        marker
    );
    assert_eq!(text("English", "中文"), "中文");
    set_chinese(false);
    let input = console.input;
    let old_mode = console.input_mode;
    drop(console);
    let mut restored = 0;
    // SAFETY: input is borrowed from the test host and still valid after guard drop.
    assert_ne!(unsafe { GetConsoleMode(input, &mut restored) }, 0);
    assert_eq!(old_mode, restored);
}

#[cfg(test)]
pub(super) fn probe_stop_key(session: &crate::Session) {
    let console = Console::open().unwrap();
    let record = INPUT_RECORD {
        EventType: KEY_EVENT as u16,
        Event: INPUT_RECORD_0 {
            KeyEvent: KEY_EVENT_RECORD {
                bKeyDown: 1,
                wRepeatCount: 1,
                wVirtualKeyCode: 0x5a,
                dwControlKeyState: LEFT_CTRL_PRESSED,
                uChar: KEY_EVENT_RECORD_0 { UnicodeChar: 26 },
                ..Default::default()
            },
        },
    };
    let mut written = 0;
    assert_ne!(
        unsafe { WriteConsoleInputW(console.input, &record, 1, &mut written) },
        0
    );
    assert_eq!(console.poll_key().unwrap(), Some(Key::StopGame));
    assert!(session.stop().unwrap());
}

#[cfg(test)]
pub(super) fn probe_running_menu(session: crate::Session) {
    let input = unsafe { GetStdHandle(STD_INPUT_HANDLE) } as usize;
    let writer = std::thread::spawn(move || {
        for (virtual_key, character, modifiers) in [
            (9, 9, 0),
            (0x5a, 26, LEFT_CTRL_PRESSED),
            (0x30, 0x30, 0),
            (0x0d, 13, 0),
        ] {
            // Let the menu enter raw input mode before injecting a real console record.
            std::thread::sleep(std::time::Duration::from_millis(150));
            let record = INPUT_RECORD {
                EventType: KEY_EVENT as u16,
                Event: INPUT_RECORD_0 {
                    KeyEvent: KEY_EVENT_RECORD {
                        bKeyDown: 1,
                        wRepeatCount: 1,
                        wVirtualKeyCode: virtual_key,
                        dwControlKeyState: modifiers,
                        uChar: KEY_EVENT_RECORD_0 {
                            UnicodeChar: character,
                        },
                        ..Default::default()
                    },
                },
            };
            let mut written = 0;
            assert_ne!(
                unsafe { WriteConsoleInputW(input as HANDLE, &record, 1, &mut written) },
                0
            );
        }
    });
    let mut active = Some(session);
    assert!(run_inner(Options::default(), false, &mut active).unwrap());
    assert!(
        active.is_none(),
        "Ctrl+Z must stop and release the running session before Exit"
    );
    writer.join().unwrap();
    set_chinese(false);
}

#[cfg(test)]
pub(super) fn probe_exit_menu(session: crate::Session) {
    let input = unsafe { GetStdHandle(STD_INPUT_HANDLE) } as usize;
    let writer = std::thread::spawn(move || {
        for (virtual_key, character) in [(0x30, 0x30), (0x0d, 13)] {
            std::thread::sleep(std::time::Duration::from_millis(150));
            let record = INPUT_RECORD {
                EventType: KEY_EVENT as u16,
                Event: INPUT_RECORD_0 {
                    KeyEvent: KEY_EVENT_RECORD {
                        bKeyDown: 1,
                        wRepeatCount: 1,
                        wVirtualKeyCode: virtual_key,
                        uChar: KEY_EVENT_RECORD_0 {
                            UnicodeChar: character,
                        },
                        ..Default::default()
                    },
                },
            };
            let mut written = 0;
            assert_ne!(
                unsafe { WriteConsoleInputW(input as HANDLE, &record, 1, &mut written) },
                0
            );
        }
    });
    let mut active = Some(session);
    assert!(run_inner(Options::default(), false, &mut active).unwrap());
    assert!(active.is_some(), "menu exit must not stop the game");
    drop(active.take());
    writer.join().unwrap();
}

impl Drop for Console {
    fn drop(&mut self) {
        // SAFETY: restore the console modes saved by this guard; handles are borrowed.
        unsafe {
            SetConsoleMode(self.input, self.input_mode);
        }
    }
}

fn render(
    state: &MenuState,
    options: &Options,
    session: Option<&crate::Session>,
) -> io::Result<()> {
    println!("\n{}", text("Endfield Launcher", "终末地启动器"));
    println!("{}: {}\n", text("Game", "游戏路径"), options.game.display());
    match session {
        Some(session) => println!(
            "{}: PID {}\n",
            text("Running", "运行中"),
            session.child.id()
        ),
        None => println!("{}\n", text("Game is not running.", "游戏未运行。")),
    }
    for label in [
        text(
            "1  Check Games.exe updates and start game",
            "1  检查 Games.exe 更新并启动游戏",
        ),
        text(
            "2  Check / download Games.exe only",
            "2  仅检查并下载 Games.exe",
        ),
        text(
            "3  Preview launch (no network or processes)",
            "3  预览启动（不联网、不启动进程）",
        ),
        text(
            "0  Exit launcher (game keeps running)",
            "0  退出启动器（游戏继续运行）",
        ),
    ] {
        println!("  {label}");
    }
    println!(
        "\n{}",
        text(
            "Type 1 / 2 / 3 / 0, then press Enter. Backspace edits input.",
            "输入 1 / 2 / 3 / 0 后按 Enter 执行，可按 Backspace 修改。"
        )
    );
    println!(
        "{}",
        text(
            "Tab: 切换为中文   Esc / Ctrl+C: exit menu",
            "Tab：Switch to English   Esc / Ctrl+C：退出菜单"
        )
    );
    println!(
        "{}",
        text(
            "Ctrl+Z: close this launcher's game and keep the menu open.",
            "Ctrl+Z：关闭本工具启动的游戏，保留菜单。"
        )
    );
    prompt(state)
}

fn prompt(state: &MenuState) -> io::Result<()> {
    print!("{}{}", text("Choice > ", "请输入编号 > "), state.input);
    io::stdout().flush()
}

pub(super) fn run(options: Options, auto_start: bool) -> Result<bool, String> {
    // Validate the terminal before any automatic launch.
    drop(Console::open()?);
    let mut session = None;
    let result = run_inner(options, auto_start, &mut session);
    // Dropping the visible session leaves the game and its independent guardian
    // running, including when console input/output fails.
    drop(session.take());
    result
}

fn run_inner(
    options: Options,
    auto_start: bool,
    session: &mut Option<crate::Session>,
) -> Result<bool, String> {
    let mut state = MenuState::default();
    if auto_start {
        crate::logging::info(
            "正在检查 Games.exe 并启动游戏。",
            "Checking Games.exe and starting the game.",
        );
        io::stdout().flush().map_err(|error| error.to_string())?;
        match crate::execute(options.clone()) {
            Ok(crate::Outcome::Started(started)) => {
                crate::logging::info(
                    format!("游戏进程已创建，进入菜单，PID：{}。", started.child.id()),
                    format!(
                        "Game process created; entering menu, PID: {}.",
                        started.child.id()
                    ),
                );
                *session = Some(started);
            }
            Ok(crate::Outcome::Finished(_)) => {}
            Err(error) => crate::logging::error(
                format!("启动失败，详细信息：{error}"),
                format!("Startup failed: {error}"),
            ),
        }
    }
    loop {
        let console = Console::open()?;
        let mut redraw = true;
        let selection = loop {
            if session
                .as_ref()
                .map(|active| active.child.try_wait())
                .transpose()?
                .flatten()
                .is_some()
            {
                println!();
                let ended = session.take().unwrap();
                ended.finish()?;
                crate::logging::info(
                    "游戏已退出，可以再次启动。",
                    "Game exited. You can launch it again.",
                );
                redraw = true;
            }
            if redraw {
                set_chinese(state.chinese);
                render(&state, &options, session.as_ref()).map_err(|error| error.to_string())?;
                redraw = false;
            }
            let Some(key) = console.poll_key()? else {
                continue;
            };
            if key == Key::StopGame {
                println!();
                state.input.clear();
                if let Some(active) = session.take() {
                    active.stop()?;
                    crate::logging::info("已通过 Ctrl+Z 关闭游戏。", "Game stopped by Ctrl+Z.");
                }
                redraw = true;
                continue;
            }
            let previous_len = state.input.len();
            let action = state.handle(key);
            match key {
                Key::Enter | Key::Tab | Key::Exit => println!(),
                Key::Character(character) if state.input.len() > previous_len => {
                    print!("{character}")
                }
                Key::Backspace if state.input.len() < previous_len => print!("\x08 \x08"),
                _ => {}
            }
            io::stdout().flush().map_err(|error| error.to_string())?;
            match action {
                Ok(Some(selection)) => break selection,
                Err(()) => {
                    crate::logging::error(
                        "无效输入，请输入 1、2、3 或 0。",
                        "Invalid choice. Enter 1, 2, 3 or 0.",
                    );
                    prompt(&state).map_err(|error| error.to_string())?;
                }
                Ok(None) => {}
            }
            redraw = key == Key::Tab;
        };
        drop(console);
        if selection == 3 {
            return Ok(true);
        }
        if session.is_some() && selection != 2 {
            crate::logging::error(
                "游戏正在运行，请先按 Ctrl+Z 关闭，再启动或检查更新。",
                "Game is running. Close it with Ctrl+Z before starting another operation.",
            );
            continue;
        }
        let mut selected_options = options.clone();
        selected_options.action = [Action::Launch, Action::UpdateOnly, Action::DryRun][selection];
        selected_options.wait = true;
        println!();
        match crate::execute(selected_options) {
            Ok(crate::Outcome::Started(started)) => {
                *session = Some(started);
                continue;
            }
            Ok(crate::Outcome::Finished(true)) => {
                crate::logging::info("操作已结束。", "Operation finished.")
            }
            Ok(crate::Outcome::Finished(false)) => crate::logging::error(
                "游戏异常退出，请查看上方信息。",
                "Game exited with an error; see the details above.",
            ),
            Err(error) => crate::logging::error(
                format!("操作失败，详细信息：{error}"),
                format!("Operation failed: {error}"),
            ),
        }
    }
}

pub(super) fn wait_for_session(session: &crate::Session) -> Result<bool, String> {
    // Redirected/non-console callers retain ordinary process waiting.
    let Ok(console) = Console::open() else {
        return session.finish();
    };
    println!(
        "{}",
        text(
            "Ctrl+Z: close the game. Ctrl+C / Esc: exit launcher; game keeps running.",
            "Ctrl+Z：关闭游戏。Ctrl+C / Esc：退出启动器，游戏继续运行。"
        )
    );
    loop {
        if session.child.try_wait()?.is_some() {
            return session.finish();
        }
        match console.poll_key()? {
            Some(Key::StopGame) => return session.stop(),
            Some(Key::Exit) => return Ok(true),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_buffers_input_and_only_submits_valid_choices_on_enter() {
        let mut state = MenuState::default();
        assert_eq!(state.handle(Key::Character('2')), Ok(None));
        assert_eq!(state.handle(Key::Tab), Ok(None));
        assert!(state.chinese);
        assert_eq!(state.input, "2");
        assert_eq!(state.handle(Key::Tab), Ok(None));
        assert!(!state.chinese);
        assert_eq!(state.handle(Key::Enter), Ok(Some(1)));
        assert!(state.input.is_empty());
        assert_eq!(state.handle(Key::Enter), Err(()));
        state.handle(Key::Character('1')).unwrap();
        state.handle(Key::Character('0')).unwrap();
        assert_eq!(state.handle(Key::Enter), Err(()));
        state.handle(Key::Character('9')).unwrap();
        state.handle(Key::Backspace).unwrap();
        state.handle(Key::Character('3')).unwrap();
        assert_eq!(state.handle(Key::Enter), Ok(Some(2)));
        state.handle(Key::Character('0')).unwrap();
        assert_eq!(state.handle(Key::Enter), Ok(Some(3)));
        assert_eq!(state.handle(Key::Exit), Ok(Some(3)));
    }

    #[test]
    fn console_decoder_ignores_key_up_and_recognizes_tab_and_ctrl_c() {
        let mut key = KEY_EVENT_RECORD {
            bKeyDown: 1,
            wVirtualKeyCode: 9,
            ..Default::default()
        };
        assert_eq!(decode_key(key), Some(Key::Tab));
        key.bKeyDown = 0;
        assert_eq!(decode_key(key), None);
        key.bKeyDown = 1;
        key.wVirtualKeyCode = 0x43;
        key.dwControlKeyState = LEFT_CTRL_PRESSED;
        assert_eq!(decode_key(key), Some(Key::Exit));
        key.wVirtualKeyCode = 0x5a;
        assert_eq!(decode_key(key), Some(Key::StopGame));
    }
}
