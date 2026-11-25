//! The terminal interface of fastlock.
//!
//! A thin layer over `fastlock-core`: it lists files, shows what the core
//! finds in them, asks for what is needed to open them, and runs verify and
//! decrypt on a worker thread. It contains no encryption logic.
//!
//! The command-line program calls [`run`] for `fastlock tui [path]`.

#![forbid(unsafe_code)]

mod app;
mod field;
mod layout;
mod listing;
mod theme;
mod ui;
mod worker;

use std::collections::VecDeque;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::JoinHandle;
use std::time::Duration;

use fastlock_core::{Error, ErrorKind, IdentityFile};
use ratatui::crossterm::event::{
    self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
    Event as TermEvent, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind,
};
use ratatui::crossterm::execute;
use ratatui::DefaultTerminal;

use crate::app::{App, Effect, Event, Key};
use crate::worker::Message;

pub use crate::theme::Theme;

/// How long to wait for input while a job runs, so progress shows smoothly.
const BUSY_TICK: Duration = Duration::from_millis(50);
/// How long to wait for input otherwise.
const IDLE_TICK: Duration = Duration::from_millis(250);

/// Runs the terminal interface until the person quits.
///
/// `start` is the folder the file list starts in; a file selects that file in
/// its folder. Without it, the current folder is used.
///
/// The terminal is always restored: on a normal exit, on an error, and on a
/// panic.
pub fn run(start: Option<PathBuf>) -> io::Result<()> {
    let cwd = std::env::current_dir()?;
    let (dir, select) = start_location(start, &cwd);
    let home = home_dir();
    // A relative path in the variable means "relative to where fastlock was
    // started", so it is made absolute here. Paths typed inside the interface
    // are resolved from the folder shown in the list instead.
    let env_key = std::env::var("FASTLOCK_KEY")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(|value| {
            let path = std::path::Path::new(&value);
            if path.is_absolute() || value.starts_with('~') {
                value
            } else {
                cwd.join(path).to_string_lossy().into_owned()
            }
        });
    let mut app = App::new(dir, home, env_key);

    // `try_init` enables raw mode, enters the alternate screen and installs a
    // panic hook that restores both. If it fails half way, undo what it did.
    let mut terminal = match ratatui::try_init() {
        Ok(terminal) => terminal,
        Err(err) => {
            ratatui::restore();
            return Err(err);
        }
    };
    let _guard = TerminalGuard;
    install_panic_hook();
    execute!(io::stdout(), EnableMouseCapture, EnableBracketedPaste)?;

    let (tx, rx) = mpsc::channel();
    let mut runner = Runner { tx, worker: None };
    let result = event_loop(&mut terminal, &mut app, &mut runner, &rx, select);

    // Never leave a job running, and its secrets in memory, behind.
    app.cancel_job();
    if let Some(worker) = runner.worker.take() {
        let _ = worker.join();
    }
    result
}

fn event_loop(
    terminal: &mut DefaultTerminal,
    app: &mut App,
    runner: &mut Runner,
    rx: &Receiver<Message>,
    select: Option<String>,
) -> io::Result<()> {
    let size = terminal.size()?;
    let effects = app.update(Event::Resize(size.width, size.height));
    runner.apply(app, effects);
    let effects = app.start(select);
    runner.apply(app, effects);

    while !app.should_quit() {
        terminal.draw(|frame| ui::render(frame, &*app))?;

        let tick = if app.job_running() {
            BUSY_TICK
        } else {
            IDLE_TICK
        };
        if event::poll(tick)? {
            if let Some(event) = translate(event::read()?) {
                let effects = app.update(event);
                runner.apply(app, effects);
            }
        }
        while let Ok(message) = rx.try_recv() {
            let effects = app.update(Event::Worker(message));
            runner.apply(app, effects);
        }
    }
    Ok(())
}

/// Carries out effects, feeding their results back into the app.
struct Runner {
    tx: Sender<Message>,
    worker: Option<JoinHandle<()>>,
}

impl Runner {
    fn apply(&mut self, app: &mut App, effects: Vec<Effect>) {
        let mut queue: VecDeque<Effect> = effects.into();
        while let Some(effect) = queue.pop_front() {
            let follow = match effect {
                Effect::ListDir(dir) => {
                    let dir = fs::canonicalize(&dir).unwrap_or(dir);
                    let result = listing::read_dir(&dir)
                        .map_err(|err| format!("cannot open the folder {}: {err}", dir.display()));
                    app.update(Event::Listed { dir, result })
                }
                Effect::Inspect(path) => {
                    let result = fastlock_core::inspect(&path).map_err(|e| e.message().to_string());
                    app.update(Event::Inspected { path, result })
                }
                Effect::ReadKey(path) => {
                    let result = IdentityFile::read(&path).map_err(|e| e.message().to_string());
                    app.update(Event::KeyRead { path, result })
                }
                Effect::Start(job) => {
                    // Only one job runs at a time; an earlier one has ended.
                    if let Some(previous) = self.worker.take() {
                        let _ = previous.join();
                    }
                    match worker::spawn(job, self.tx.clone()) {
                        Ok(handle) => {
                            self.worker = Some(handle);
                            Vec::new()
                        }
                        Err(err) => {
                            let err = Error::new(
                                ErrorKind::Other,
                                format!("cannot start the work: {err}"),
                            );
                            app.update(Event::Worker(Message::Done(Err(err))))
                        }
                    }
                }
                Effect::Quit => Vec::new(),
            };
            queue.extend(follow);
        }
    }
}

/// Restores the terminal when dropped, also while unwinding from a panic.
struct TerminalGuard;

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        restore_terminal();
    }
}

fn restore_terminal() {
    let _ = execute!(io::stdout(), DisableBracketedPaste, DisableMouseCapture);
    ratatui::restore();
}

/// Adds mouse and paste mode to what ratatui's panic hook restores. It runs
/// before ratatui's hook, which then leaves raw mode and the alternate
/// screen, so the panic message is readable.
fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = execute!(io::stdout(), DisableBracketedPaste, DisableMouseCapture);
        previous(info);
    }));
}

/// Turns a terminal event into an app event. Key releases are ignored.
fn translate(event: TermEvent) -> Option<Event> {
    match event {
        TermEvent::Key(key) if key.kind != KeyEventKind::Release => {
            translate_key(key).map(Event::Key)
        }
        TermEvent::Mouse(mouse) => match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => Some(Event::Click {
                column: mouse.column,
                row: mouse.row,
            }),
            MouseEventKind::ScrollDown => Some(Event::Scroll(1)),
            MouseEventKind::ScrollUp => Some(Event::Scroll(-1)),
            _ => None,
        },
        TermEvent::Resize(width, height) => Some(Event::Resize(width, height)),
        TermEvent::Paste(text) => Some(Event::Paste(text)),
        _ => None,
    }
}

fn translate_key(key: KeyEvent) -> Option<Key> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    Some(match key.code {
        KeyCode::Char('c') if ctrl => Key::Interrupt,
        KeyCode::Char('u') if ctrl => Key::ClearLine,
        KeyCode::Char(_) if ctrl => return None,
        KeyCode::Char(c) => Key::Char(c),
        KeyCode::Enter => Key::Enter,
        KeyCode::Esc => Key::Esc,
        KeyCode::Backspace => Key::Backspace,
        KeyCode::Delete => Key::Delete,
        KeyCode::Left => Key::Left,
        KeyCode::Right => Key::Right,
        KeyCode::Up => Key::Up,
        KeyCode::Down => Key::Down,
        KeyCode::Home => Key::Home,
        KeyCode::End => Key::End,
        KeyCode::PageUp => Key::PageUp,
        KeyCode::PageDown => Key::PageDown,
        _ => return None,
    })
}

/// The folder to start in, and the file to select in it.
fn start_location(start: Option<PathBuf>, cwd: &Path) -> (PathBuf, Option<String>) {
    let Some(start) = start else {
        return (cwd.to_path_buf(), None);
    };
    let start = if start.is_relative() {
        cwd.join(start)
    } else {
        start
    };
    if start.is_file() {
        let name = start.file_name().map(|n| n.to_string_lossy().into_owned());
        let dir = start
            .parent()
            .map_or_else(|| cwd.to_path_buf(), Path::to_path_buf);
        (dir, name)
    } else {
        (start, None)
    }
}

/// The home folder, from the environment.
fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_translated() {
        let plain = |code| KeyEvent::new(code, KeyModifiers::NONE);
        assert_eq!(
            translate_key(plain(KeyCode::Char('j'))),
            Some(Key::Char('j'))
        );
        assert_eq!(translate_key(plain(KeyCode::Esc)), Some(Key::Esc));
        assert_eq!(
            translate_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)),
            Some(Key::Interrupt)
        );
        assert_eq!(
            translate_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL)),
            None
        );
    }

    #[test]
    fn key_releases_are_ignored() {
        let release = KeyEvent::new_with_kind(
            KeyCode::Char('q'),
            KeyModifiers::NONE,
            KeyEventKind::Release,
        );
        assert!(translate(TermEvent::Key(release)).is_none());
    }

    #[test]
    fn a_file_as_start_selects_it_in_its_folder() {
        let dir = std::env::temp_dir().join(format!("fastlock-tui-start-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("sample.age"), b"sample").unwrap();
        let (start, select) = start_location(Some(dir.join("sample.age")), Path::new("/"));
        let _ = fs::remove_dir_all(&dir);
        assert_eq!(start, dir);
        assert_eq!(select.as_deref(), Some("sample.age"));

        let (start, select) = start_location(None, Path::new("/sample"));
        assert_eq!(start, PathBuf::from("/sample"));
        assert_eq!(select, None);
    }
}
