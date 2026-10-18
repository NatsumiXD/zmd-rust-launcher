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
    BackTab,
    Up,
    Down,
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
        (0x09, _) if key.dwControlKeyState & SHIFT_PRESSED != 0 => Key::BackTab,
        (0x09, _) => Key::Tab,
        (0x26, _) => Key::Up,
        (0x28, _) => Key::Down,
        (_, character @ 0x20..=0x7e) => Key::Character(char::from_u32(character as u32).unwrap()),
        _ => Key::Other,
    })
}

const COMMANDS: &[&str] = &[
    "help", "start", "launch", "restart", "update", "preview", "status", "stop", "cn", "exit",
    "quit",
];
const PROMPT: &str = "endfield> ";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Command {
    Help,
    Start,
    Restart,
    Update,
    Preview,
    Status,
    Stop,
    Chinese,
    Exit,
}

fn parse_command(input: &str) -> Result<Option<Command>, String> {
    match input.trim().to_ascii_lowercase().as_str() {
        "" => Ok(None),
        "help" | "?" => Ok(Some(Command::Help)),
        "start" | "launch" | "1" => Ok(Some(Command::Start)),
        "restart" => Ok(Some(Command::Restart)),
        "update" | "2" => Ok(Some(Command::Update)),
        "preview" | "3" => Ok(Some(Command::Preview)),
        "status" => Ok(Some(Command::Status)),
        "stop" => Ok(Some(Command::Stop)),
        "cn" => Ok(Some(Command::Chinese)),
        "exit" | "quit" | "q" | "0" => Ok(Some(Command::Exit)),
        _ => Err("Unknown command. Type help to list commands.".into()),
    }
}

#[derive(Default)]
struct MenuState {
    input: String,
    history: Vec<String>,
    history_index: Option<usize>,
    draft: String,
    completion: Option<Completion>,
}

struct Completion {
    candidates: Vec<&'static str>,
    selected: usize,
}

impl MenuState {
    fn candidates(&self) -> Vec<&'static str> {
        if let Some(completion) = &self.completion {
            return completion.candidates.clone();
        }
        let prefix = self.input.trim_start().to_ascii_lowercase();
        COMMANDS
            .iter()
            .copied()
            .filter(|command| command.starts_with(&prefix))
            .collect()
    }

    fn complete(&mut self, reverse: bool) {
        if let Some(completion) = &mut self.completion {
            let count = completion.candidates.len();
            completion.selected = if reverse {
                (completion.selected + count - 1) % count
            } else {
                (completion.selected + 1) % count
            };
        } else {
            let candidates = self.candidates();
            if candidates.is_empty() {
                return;
            }
            let selected = if reverse { candidates.len() - 1 } else { 0 };
            self.completion = Some(Completion {
                candidates,
                selected,
            });
        }
        let completion = self.completion.as_ref().unwrap();
        self.input = completion.candidates[completion.selected].into();
    }

    fn hint(&self) -> String {
        let candidates = self.candidates();
        if candidates.is_empty() {
            return "  [No matching command; help lists commands]".into();
        }
        if candidates.len() == 1 && self.input.eq_ignore_ascii_case(candidates[0]) {
            return "  [Enter to run]".into();
        }
        let choices: Vec<_> = candidates
            .iter()
            .enumerate()
            .map(|(index, candidate)| {
                if self
                    .completion
                    .as_ref()
                    .is_some_and(|completion| completion.selected == index)
                {
                    format!("<{candidate}>")
                } else {
                    candidate.to_string()
                }
            })
            .collect();
        format!("  [{}]", choices.join(" | "))
    }

    fn handle(&mut self, key: Key) -> Result<Option<Command>, String> {
        if !matches!(key, Key::Tab | Key::BackTab | Key::Other) {
            self.completion = None;
        }
        match key {
            Key::Character(character) if self.input.len() < 256 => self.input.push(character),
            Key::Character(_) => {}
            Key::Backspace => {
                self.input.pop();
            }
            Key::Tab => self.complete(false),
            Key::BackTab => self.complete(true),
            Key::Up => {
                if !self.history.is_empty() {
                    let index = match self.history_index {
                        Some(index) => index.saturating_sub(1),
                        None => {
                            self.draft = self.input.clone();
                            self.history.len() - 1
                        }
                    };
                    self.history_index = Some(index);
                    self.input = self.history[index].clone();
                }
            }
            Key::Down => {
                if let Some(index) = self.history_index {
                    if index + 1 < self.history.len() {
                        self.history_index = Some(index + 1);
                        self.input = self.history[index + 1].clone();
                    } else {
                        self.history_index = None;
                        self.input = std::mem::take(&mut self.draft);
                    }
                }
            }
            Key::Enter => {
                let input = std::mem::take(&mut self.input);
                self.history_index = None;
                self.draft.clear();
                if !input.trim().is_empty() && self.history.last() != Some(&input) {
                    self.history.push(input.clone());
                    if self.history.len() > 50 {
                        self.history.remove(0);
                    }
                }
                return parse_command(&input);
            }
            Key::Exit => return Ok(Some(Command::Exit)),
            Key::Other => {}
            Key::StopGame => {}
        }
        Ok(None)
    }
}

fn console_input_mode(original: u32) -> u32 {
    // Leave selection/scrollback with the console host. Previously Quick Edit
    // was disabled while mouse input stayed enabled; wheel records were then
    // consumed and ignored by our ReadConsoleInput loop.
    (original | ENABLE_EXTENDED_FLAGS)
        & !(ENABLE_LINE_INPUT
            | ENABLE_ECHO_INPUT
            | ENABLE_PROCESSED_INPUT
            | ENABLE_MOUSE_INPUT
            | ENABLE_VIRTUAL_TERMINAL_INPUT)
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
        let mode = console_input_mode(input_mode);
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
fn probe_native_wheel(output: HANDLE) {
    #[repr(C)]
    #[derive(Default)]
    struct Rect {
        left: i32,
        top: i32,
        right: i32,
        bottom: i32,
    }
    #[link(name = "user32")]
    unsafe extern "system" {
        fn GetWindowRect(window: HANDLE, rect: *mut Rect) -> i32;
        fn PostMessageW(window: HANDLE, message: u32, wparam: usize, lparam: isize) -> i32;
    }
    let mut info = CONSOLE_SCREEN_BUFFER_INFO::default();
    assert_ne!(unsafe { GetConsoleScreenBufferInfo(output, &mut info) }, 0);
    let rows = info.srWindow.Bottom - info.srWindow.Top + 1;
    for number in 0..rows + 30 {
        println!("scrollback-probe-{number}");
    }
    io::stdout().flush().unwrap();
    assert_ne!(unsafe { GetConsoleScreenBufferInfo(output, &mut info) }, 0);
    let before = info.srWindow.Top;
    let window = unsafe { GetConsoleWindow() };
    let mut rect = Rect::default();
    assert_ne!(unsafe { GetWindowRect(window, &mut rect) }, 0);
    let x = ((rect.left + rect.right) / 2) as u16 as usize;
    let y = ((rect.top + rect.bottom) / 2) as u16 as usize;
    assert_ne!(
        unsafe { PostMessageW(window, 0x020a, 120usize << 16, (x | (y << 16)) as isize) },
        0
    );
    std::thread::sleep(std::time::Duration::from_millis(200));
    assert_ne!(unsafe { GetConsoleScreenBufferInfo(output, &mut info) }, 0);
    let after = info.srWindow.Top;
    let record = serde_json::json!({"scrollback_rows": info.dwSize.Y, "viewport_top_before_wheel": before, "viewport_top_after_wheel": after, "native_wheel_scrolled_up": after < before});
    std::fs::write(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("analysis/terminal-scroll-probe.json"),
        serde_json::to_vec_pretty(&record).unwrap(),
    )
    .unwrap();
    assert!(
        after < before,
        "native wheel did not scroll the private console: {before} -> {after}"
    );
}

#[cfg(test)]
pub(super) fn probe_console_keys() {
    // The caller creates an isolated, hidden console for this test host.
    let console = Console::open().unwrap();
    let mut state = MenuState::default();
    for (virtual_key, character, expected) in [
        (0x52, 0x72, Key::Character('r')),
        (0x45, 0x65, Key::Character('e')),
        (9, 9, Key::Tab),
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
            assert_eq!(choice, Ok(Some(Command::Restart)));
        } else {
            assert_eq!(choice, Ok(None));
        }
    }
    let mut mode = 0;
    assert_ne!(unsafe { GetConsoleMode(console.input, &mut mode) }, 0);
    assert_eq!(mode & ENABLE_MOUSE_INPUT, 0);
    assert_eq!(
        mode & ENABLE_QUICK_EDIT_MODE,
        console.input_mode & ENABLE_QUICK_EDIT_MODE
    );
    prepare_scrollback();
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
    banner(&Options::default());
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
    state.input = "st".into();
    prompt(&state).unwrap();
    assert_ne!(
        unsafe { GetConsoleScreenBufferInfo(output, &mut screen) },
        0
    );
    assert_eq!(screen.dwCursorPosition.X as usize, PROMPT.len() + 2);
    let position = COORD {
        X: 0,
        Y: screen.dwCursorPosition.Y,
    };
    let mut line = vec![0u16; screen.dwSize.X as usize];
    assert_ne!(
        unsafe {
            ReadConsoleOutputCharacterW(
                output,
                line.as_mut_ptr(),
                line.len() as u32,
                position,
                &mut read,
            )
        },
        0
    );
    assert!(String::from_utf16_lossy(&line[..read as usize]).contains("[start | status | stop]"));
    state.handle(Key::Tab).unwrap();
    prompt(&state).unwrap();
    assert_ne!(
        unsafe {
            ReadConsoleOutputCharacterW(
                output,
                line.as_mut_ptr(),
                line.len() as u32,
                position,
                &mut read,
            )
        },
        0
    );
    assert!(String::from_utf16_lossy(&line[..read as usize]).contains("[<start> | status | stop]"));
    state.handle(Key::BackTab).unwrap();
    assert_eq!(state.input, "stop");
    finish_input(&state).unwrap();
    assert_ne!(
        unsafe {
            ReadConsoleOutputCharacterW(
                output,
                line.as_mut_ptr(),
                line.len() as u32,
                position,
                &mut read,
            )
        },
        0
    );
    let committed = String::from_utf16_lossy(&line[..read as usize]);
    assert!(committed.starts_with("endfield> stop"));
    assert!(
        !committed.contains('|'),
        "candidate hints must be removed before logs/command output"
    );
    probe_native_wheel(output);
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
            (0x43, 0x63, 0),
            (9, 9, 0),
            (0x0d, 13, 0),
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

#[cfg(test)]
pub(super) fn probe_launch_commands(
    options: Options,
    mut session: Option<crate::Session>,
    commands: &[&str],
) -> crate::Session {
    let input = unsafe { GetStdHandle(STD_INPUT_HANDLE) } as usize;
    let commands: Vec<String> = commands.iter().map(|text| text.to_string()).collect();
    let writer = std::thread::spawn(move || {
        for command in commands {
            std::thread::sleep(std::time::Duration::from_millis(500));
            let records: Vec<_> = command
                .encode_utf16()
                .chain(Some(13))
                .map(|character| INPUT_RECORD {
                    EventType: KEY_EVENT as u16,
                    Event: INPUT_RECORD_0 {
                        KeyEvent: KEY_EVENT_RECORD {
                            bKeyDown: 1,
                            wRepeatCount: 1,
                            wVirtualKeyCode: if character == 13 { 0x0d } else { 0 },
                            uChar: KEY_EVENT_RECORD_0 {
                                UnicodeChar: character,
                            },
                            ..Default::default()
                        },
                    },
                })
                .collect();
            let mut written = 0;
            assert_ne!(
                unsafe {
                    WriteConsoleInputW(
                        input as HANDLE,
                        records.as_ptr(),
                        records.len() as u32,
                        &mut written,
                    )
                },
                0
            );
            assert_eq!(written as usize, records.len());
        }
    });
    assert!(run_inner(options, false, &mut session).unwrap());
    writer.join().unwrap();
    session.expect("start/restart must leave a running fixture after exiting the console")
}

impl Drop for Console {
    fn drop(&mut self) {
        // SAFETY: restore the console modes saved by this guard; handles are borrowed.
        unsafe {
            SetConsoleMode(self.input, self.input_mode);
        }
    }
}

fn prepare_scrollback() {
    let mut processes = [0u32; 8];
    // Resize only our own newly-created console, not a caller's PowerShell tab.
    if unsafe { GetConsoleProcessList(processes.as_mut_ptr(), 8) } != 1
        || processes[0] != std::process::id()
    {
        return;
    }
    let output = unsafe { GetStdHandle(STD_OUTPUT_HANDLE) };
    let mut info = CONSOLE_SCREEN_BUFFER_INFO::default();
    if unsafe { GetConsoleScreenBufferInfo(output, &mut info) } != 0 && info.dwSize.Y < 20_000 {
        // Legacy conhost uses this buffer for scrollback. Modern terminal hosts
        // may reject resizing and keep managing their own scrollback instead.
        unsafe {
            SetConsoleScreenBufferSize(
                output,
                COORD {
                    X: info.dwSize.X,
                    Y: 20_000,
                },
            );
        }
    }
}

fn banner(options: &Options) {
    println!("\nEndfield Launcher {}", env!("CARGO_PKG_VERSION"));
    println!("Game: {}", options.game.display());
    println!("Type help for commands; cn prints earlier logs in Chinese.");
    println!("Ctrl+Z: stop game. Exit/close launcher: keep game running.\n");
}

fn help() {
    println!("start    Check Games.exe updates and start the game (alias: 1)");
    println!("restart  Stop this session's game, check updates and start again");
    println!("update   Check/download Games.exe only (alias: 2)");
    println!("preview  Preview launch without network/processes (alias: 3)");
    println!("status   Show this session's game status");
    println!("stop     Stop this session's game (shortcut: Ctrl+Z)");
    println!("cn       Print a Chinese translation of earlier session logs");
    println!("help     Show commands");
    println!("exit     Exit launcher and keep game running (aliases: 0, q)");
    println!("Candidates appear as you type. Tab / Shift+Tab: cycle completions.");
    println!("Enter: run the typed command. Up/Down: command history.\n");
}

fn status(options: &Options, session: Option<&crate::Session>) {
    println!("Game: {}", options.game.display());
    if let Some(session) = session {
        println!(
            "Running (owned by this session), PID: {}",
            session.child.id()
        );
    } else {
        println!("No running game owned by this session.");
    }
}

fn prompt(state: &MenuState) -> io::Result<()> {
    render_input(state, true)
}

fn input_line(state: &MenuState, width: usize, show_hint: bool) -> (String, String) {
    // Keep one spare column to avoid automatic wrapping. All input/hints are ASCII.
    let available = width.saturating_sub(PROMPT.len() + 1);
    let visible = if state.input.len() <= available {
        state.input.clone()
    } else if available > 1 {
        format!("<{}", &state.input[state.input.len() - available + 1..])
    } else {
        String::new()
    };
    let hint = if show_hint {
        state.hint()
    } else {
        String::new()
    };
    let hint_space = available.saturating_sub(visible.len());
    let hint = if hint.len() > hint_space && hint_space >= 3 {
        format!("{}...", &hint[..hint_space - 3])
    } else {
        hint.chars().take(hint_space).collect()
    };
    (format!("{PROMPT}{visible}"), hint)
}

fn render_input(state: &MenuState, show_hint: bool) -> io::Result<()> {
    let handle = unsafe { GetStdHandle(STD_OUTPUT_HANDLE) };
    let mut info = CONSOLE_SCREEN_BUFFER_INFO::default();
    let console_output = unsafe { GetConsoleScreenBufferInfo(handle, &mut info) } != 0;
    let width = if console_output {
        info.dwSize.X.max(1) as usize
    } else {
        120
    };
    // Extremely narrow windows cannot fit the prompt; omit hints in that case.
    if width <= PROMPT.len() + 1 {
        print!(
            "\r{}",
            PROMPT
                .chars()
                .take(width.saturating_sub(1))
                .collect::<String>()
        );
        return io::stdout().flush();
    }
    let (typed, hint) = input_line(state, width, show_hint);
    let padding = width.saturating_sub(1 + typed.len() + hint.len());
    let mut output = io::stdout().lock();
    write!(output, "\r{typed}")?;
    output.flush()?;
    // Use the console's current background and restore the original colors.
    if console_output {
        unsafe { SetConsoleTextAttribute(handle, (info.wAttributes & !0x0f) | 0x08) };
    }
    let written = write!(output, "{hint}{}", " ".repeat(padding)).and_then(|_| output.flush());
    if console_output {
        unsafe { SetConsoleTextAttribute(handle, info.wAttributes) };
    }
    written?;
    // Leave the cursor before the hint, ready for the next character. Only this
    // input line is rewritten; scrollback and earlier logs are left intact.
    write!(output, "\r{typed}")?;
    output.flush()
}

fn finish_input(state: &MenuState) -> io::Result<()> {
    render_input(state, false)?;
    println!();
    Ok(())
}

fn stop(session: &mut Option<crate::Session>) -> Result<(), String> {
    if let Some(active) = session.as_ref() {
        active.stop()?;
        session.take();
    } else {
        crate::logging::info(
            "当前会话没有正在运行的游戏。",
            "No running game owned by this session.",
        );
    }
    Ok(())
}

pub(super) fn run(options: Options, auto_start: bool) -> Result<bool, String> {
    drop(Console::open()?);
    prepare_scrollback();
    let mut session = None;
    let result = run_inner(options, auto_start, &mut session);
    drop(session.take()); // An independent guardian keeps the game/official parent alive.
    result
}

fn run_inner(
    options: Options,
    auto_start: bool,
    session: &mut Option<crate::Session>,
) -> Result<bool, String> {
    if auto_start {
        crate::logging::info(
            "正在检查 Games.exe 并启动游戏。",
            "Checking Games.exe and starting the game.",
        );
        match crate::execute(options.clone()) {
            Ok(crate::Outcome::Started(started)) => {
                crate::logging::info(
                    format!(
                        "游戏进程已创建，进入命令终端，PID：{}。",
                        started.child.id()
                    ),
                    format!(
                        "Game process created; entering command console, PID: {}.",
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
    banner(&options);
    let mut state = MenuState::default();
    loop {
        let console = Console::open()?;
        prompt(&state).map_err(|error| error.to_string())?;
        let command = loop {
            if session
                .as_ref()
                .map(|active| active.child.try_wait())
                .transpose()?
                .flatten()
                .is_some()
            {
                finish_input(&state).map_err(|error| error.to_string())?;
                session.take().unwrap().finish()?;
                prompt(&state).map_err(|error| error.to_string())?;
            }
            let Some(key) = console.poll_key()? else {
                continue;
            };
            if key == Key::StopGame {
                finish_input(&state).map_err(|error| error.to_string())?;
                state.input.clear();
                state.completion = None;
                stop(session)?;
                prompt(&state).map_err(|error| error.to_string())?;
                continue;
            }
            if matches!(key, Key::Enter | Key::Exit) {
                finish_input(&state).map_err(|error| error.to_string())?;
            }
            let result = state.handle(key);
            if matches!(
                key,
                Key::Character(_) | Key::Backspace | Key::Up | Key::Down | Key::Tab | Key::BackTab
            ) {
                prompt(&state).map_err(|error| error.to_string())?;
            }
            match result {
                Ok(Some(command)) => break command,
                Ok(None) if key == Key::Enter => {
                    prompt(&state).map_err(|error| error.to_string())?;
                }
                Err(error) => {
                    crate::logging::error("未知指令，请输入 help 查看可用命令。", error);
                    prompt(&state).map_err(|error| error.to_string())?;
                }
                _ => {}
            }
        };
        drop(console);
        match command {
            Command::Exit => return Ok(true),
            Command::Help => {
                help();
                continue;
            }
            Command::Status => {
                status(&options, session.as_ref());
                continue;
            }
            Command::Chinese => {
                crate::logging::print_chinese_history().map_err(|error| error.to_string())?;
                continue;
            }
            Command::Stop => {
                stop(session)?;
                continue;
            }
            Command::Restart => {
                crate::logging::info(
                    "正在重启游戏：关闭当前会话的游戏，完成清理后重新检查并启动。",
                    "Restarting game: stopping this session's game before checking updates and starting again.",
                );
                if session.is_some() {
                    if let Err(error) = stop(session) {
                        crate::logging::error(
                            format!("重启已中止：无法确认旧会话已清理，详细信息：{error}"),
                            format!(
                                "Restart aborted: previous session cleanup was not confirmed: {error}"
                            ),
                        );
                        continue;
                    }
                } else {
                    crate::logging::info(
                        "当前会话没有正在运行的游戏，将直接启动。",
                        "No running game owned by this session; starting the game.",
                    );
                }
            }
            Command::Start | Command::Update if session.is_some() => {
                crate::logging::error(
                    "游戏正在运行，请先用 stop 或 Ctrl+Z 关闭。",
                    "Game is running. Use stop or Ctrl+Z before starting/updating.",
                );
                continue;
            }
            _ => {}
        }
        let mut selected_options = options.clone();
        selected_options.action = match command {
            Command::Start | Command::Restart => Action::Launch,
            Command::Update => Action::UpdateOnly,
            Command::Preview => Action::DryRun,
            _ => unreachable!(),
        };
        selected_options.wait = true;
        match crate::execute(selected_options) {
            Ok(crate::Outcome::Started(started)) => *session = Some(started),
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
    println!("Ctrl+Z: stop game. Ctrl+C / Esc: exit launcher; game keeps running.");
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

    fn enter(state: &mut MenuState, input: &str) -> Result<Option<Command>, String> {
        for character in input.chars() {
            assert_eq!(state.handle(Key::Character(character)), Ok(None));
        }
        state.handle(Key::Enter)
    }

    #[test]
    fn commands_require_enter_and_keep_numeric_aliases() {
        let mut state = MenuState::default();
        assert_eq!(enter(&mut state, "cn"), Ok(Some(Command::Chinese)));
        assert_eq!(enter(&mut state, " StaRt "), Ok(Some(Command::Start)));
        assert_eq!(enter(&mut state, "launch"), Ok(Some(Command::Start)));
        assert_eq!(enter(&mut state, " RESTART "), Ok(Some(Command::Restart)));
        assert_eq!(enter(&mut state, "2"), Ok(Some(Command::Update)));
        assert_eq!(enter(&mut state, "preview"), Ok(Some(Command::Preview)));
        assert_eq!(enter(&mut state, "status"), Ok(Some(Command::Status)));
        assert_eq!(enter(&mut state, "stop"), Ok(Some(Command::Stop)));
        assert_eq!(enter(&mut state, "0"), Ok(Some(Command::Exit)));
        assert_eq!(enter(&mut state, ""), Ok(None));
        assert!(enter(&mut state, "rm -rf something").is_err());
        state.handle(Key::Character('9')).unwrap();
        state.handle(Key::Backspace).unwrap();
        assert_eq!(enter(&mut state, "help"), Ok(Some(Command::Help)));
    }

    #[test]
    fn completions_cycle_original_candidates_until_input_changes() {
        let mut state = MenuState {
            input: "st".into(),
            ..Default::default()
        };
        assert_eq!(state.hint(), "  [start | status | stop]");
        state.handle(Key::Tab).unwrap();
        assert_eq!(state.input, "start");
        assert!(state.hint().contains("<start>"));
        state.handle(Key::Tab).unwrap();
        assert_eq!(state.input, "status");
        state.handle(Key::Tab).unwrap();
        assert_eq!(state.input, "stop");
        state.handle(Key::Tab).unwrap();
        assert_eq!(state.input, "start");
        state.handle(Key::BackTab).unwrap();
        assert_eq!(state.input, "stop");
        assert!(state.history.is_empty(), "Tab must not submit a command");
        state.handle(Key::Backspace).unwrap();
        assert_eq!(state.candidates(), vec!["stop"]);
        state.handle(Key::Tab).unwrap();
        assert_eq!(state.input, "stop");
        assert_eq!(state.handle(Key::Enter), Ok(Some(Command::Stop)));
        assert!(state.completion.is_none());
    }

    #[test]
    fn completion_handles_aliases_empty_unknown_and_history_input() {
        let mut state = MenuState::default();
        state.handle(Key::BackTab).unwrap();
        assert_eq!(state.input, "quit");
        state.handle(Key::Tab).unwrap();
        assert_eq!(state.input, "help");
        state.handle(Key::Enter).unwrap();
        state.input = "  LA".into();
        state.handle(Key::Tab).unwrap();
        assert_eq!(state.input, "launch");
        assert_eq!(state.handle(Key::Enter), Ok(Some(Command::Start)));
        state.input = "unknown".into();
        assert!(state.hint().contains("No matching command"));
        state.handle(Key::Tab).unwrap();
        assert_eq!(state.input, "unknown");
        state.handle(Key::Up).unwrap();
        assert_eq!(state.input, "launch");
        assert_eq!(state.hint(), "  [Enter to run]");
        state.handle(Key::Down).unwrap();
        assert_eq!(state.input, "unknown");
    }

    #[test]
    fn candidate_rendering_fits_one_line_and_keeps_full_input_for_submission() {
        let mut state = MenuState {
            input: "st".into(),
            ..Default::default()
        };
        let (typed, hint) = input_line(&state, 80, true);
        assert_eq!(typed, "endfield> st");
        assert_eq!(hint, "  [start | status | stop]");
        let (typed, hint) = input_line(&state, 25, true);
        assert!(typed.len() + hint.len() < 25);
        assert!(hint.ends_with("..."));
        state.input = "x".repeat(256);
        let (typed, hint) = input_line(&state, 40, true);
        assert!(typed.starts_with("endfield> <"));
        assert_eq!(typed.len(), 39);
        assert!(hint.is_empty());
        assert_eq!(state.input.len(), 256);
        assert!(input_line(&MenuState::default(), 80, false).1.is_empty());
    }

    #[test]
    fn command_history_restores_unsubmitted_draft() {
        let mut state = MenuState::default();
        enter(&mut state, "status").unwrap();
        enter(&mut state, "cn").unwrap();
        state.input = "pre".into();
        state.handle(Key::Up).unwrap();
        assert_eq!(state.input, "cn");
        state.handle(Key::Up).unwrap();
        assert_eq!(state.input, "status");
        state.handle(Key::Down).unwrap();
        assert_eq!(state.input, "cn");
        state.handle(Key::Down).unwrap();
        assert_eq!(state.input, "pre");
    }

    #[test]
    fn mouse_input_is_disabled_and_host_quick_edit_setting_is_preserved() {
        for quick_edit in [0, ENABLE_QUICK_EDIT_MODE] {
            let original = ENABLE_MOUSE_INPUT
                | ENABLE_LINE_INPUT
                | ENABLE_ECHO_INPUT
                | ENABLE_PROCESSED_INPUT
                | ENABLE_VIRTUAL_TERMINAL_INPUT
                | quick_edit;
            let mode = console_input_mode(original);
            assert_eq!(mode & ENABLE_MOUSE_INPUT, 0);
            assert_eq!(mode & ENABLE_QUICK_EDIT_MODE, quick_edit);
            assert_eq!(
                mode & (ENABLE_LINE_INPUT
                    | ENABLE_ECHO_INPUT
                    | ENABLE_PROCESSED_INPUT
                    | ENABLE_VIRTUAL_TERMINAL_INPUT),
                0
            );
        }
    }

    #[test]
    fn console_decoder_handles_shortcuts_and_history_keys() {
        let mut key = KEY_EVENT_RECORD {
            bKeyDown: 1,
            wVirtualKeyCode: 9,
            ..Default::default()
        };
        assert_eq!(decode_key(key), Some(Key::Tab));
        key.dwControlKeyState = SHIFT_PRESSED;
        assert_eq!(decode_key(key), Some(Key::BackTab));
        key.dwControlKeyState = 0;
        key.bKeyDown = 0;
        assert_eq!(decode_key(key), None);
        key.bKeyDown = 1;
        key.wVirtualKeyCode = 0x26;
        assert_eq!(decode_key(key), Some(Key::Up));
        key.wVirtualKeyCode = 0x43;
        key.dwControlKeyState = LEFT_CTRL_PRESSED;
        assert_eq!(decode_key(key), Some(Key::Exit));
        key.wVirtualKeyCode = 0x5a;
        assert_eq!(decode_key(key), Some(Key::StopGame));
    }
}
