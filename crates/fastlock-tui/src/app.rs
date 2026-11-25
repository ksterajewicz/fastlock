//! The state of the interface and the one step that changes it.
//!
//! [`App::update`] takes one [`Event`] and returns the [`Effect`]s the caller
//! must carry out: list a folder, inspect a file, read a key file, start a
//! job, quit. The results come back as events. Nothing in here touches the
//! terminal or the file system, so every behavior can be tested directly.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use fastlock_core::{
    default_output, CancelToken, CheckStatus, Error, ErrorKind, Format, IdentityFile, Inspection,
    Needs, SecretString, Stage,
};
use zeroize::Zeroize;

use crate::field::TextField;
use crate::layout::{keep_visible, Geometry};
use crate::listing::{display_path, file_name, resolve, Item};
use crate::theme::Theme;
use crate::worker::{Action, Job, Message, Outcome, Unlock};

/// A key, already translated from the terminal's key events.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Char(char),
    Enter,
    Esc,
    Backspace,
    Delete,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    PageUp,
    PageDown,
    /// Ctrl+C: cancel what is running and quit.
    Interrupt,
    /// Ctrl+U: empty the text field.
    ClearLine,
}

/// Everything that can change the state.
pub enum Event {
    Key(Key),
    /// A left click at a cell.
    Click {
        column: u16,
        row: u16,
    },
    /// The mouse wheel: positive moves down.
    Scroll(i32),
    Resize(u16, u16),
    Paste(String),
    /// A folder was listed.
    Listed {
        dir: PathBuf,
        result: Result<Vec<Item>, String>,
    },
    /// A file was inspected.
    Inspected {
        path: PathBuf,
        result: Result<Inspection, String>,
    },
    /// A key file was read.
    KeyRead {
        path: PathBuf,
        result: Result<IdentityFile, String>,
    },
    /// A message from the running job.
    Worker(Message),
}

/// What the caller must do after an update.
pub enum Effect {
    ListDir(PathBuf),
    Inspect(PathBuf),
    ReadKey(PathBuf),
    Start(Job),
    Quit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Home,
    Files,
    Result,
}

/// An item of the home list.
pub struct HomeItem {
    pub key: char,
    pub group: &'static str,
    pub label: &'static str,
    pub state: &'static str,
    pub action: Action,
}

/// The home list. Only what works today is listed. Shortcut letters never
/// use the navigation keys `h j k l g G q` or `/ : ?`.
pub static HOME_ITEMS: [HomeItem; 2] = [
    HomeItem {
        key: 'v',
        group: "Files",
        label: "verify",
        state: "check, write nothing",
        action: Action::Verify,
    },
    HomeItem {
        key: 'd',
        group: "Files",
        label: "decrypt",
        state: "to disk",
        action: Action::Decrypt,
    },
];

/// A line of the home list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HomeLine {
    Heading(&'static str),
    Blank,
    /// The position of the item in the filtered list.
    Item(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Info,
    Ok,
    Warning,
    Error,
}

/// The one-line message above the key hints.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    pub text: String,
    pub tone: Tone,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Prompt {
    /// `/`: filter the list.
    Search,
    /// `:`: a command.
    Command,
}

/// The line being typed in the key hint row.
pub struct Typing {
    pub prompt: Prompt,
    pub field: TextField,
}

/// A box drawn over the screen.
pub enum Modal {
    Help,
    /// Where to write the decrypted data.
    Output {
        field: TextField,
        error: Option<String>,
    },
    /// The path of a key file.
    KeyFile {
        file: String,
        field: TextField,
        error: Option<String>,
    },
    /// A passphrase, for the file or for its key file.
    Passphrase {
        purpose: String,
        field: TextField,
        error: Option<String>,
    },
    Progress,
}

/// The folder shown in the file list.
pub struct Browser {
    pub dir: PathBuf,
    pub items: Vec<Item>,
    pub loaded: bool,
    pub sel: usize,
    pub offset: usize,
}

/// The running job as the interface shows it.
pub struct JobView {
    pub action: Action,
    pub unlocking: bool,
    pub stage: Option<Stage>,
    pub done: u64,
    pub total: Option<u64>,
    pub cancelling: bool,
    cancel: CancelToken,
}

impl JobView {
    /// The finished fraction of the current stage, when its size is known.
    pub fn fraction(&self) -> Option<f64> {
        match self.total {
            Some(0) => Some(1.0),
            Some(total) => Some((self.done as f64 / total as f64).clamp(0.0, 1.0)),
            None => None,
        }
    }
}

/// The file being opened, while the forms collect what is needed.
struct Flow {
    action: Action,
    path: PathBuf,
    needs: Needs,
    output: Option<PathBuf>,
    identity: Option<Arc<IdentityFile>>,
}

/// The whole state of the interface.
pub struct App {
    pub theme: Theme,
    pub screen: Screen,
    pub action: Action,
    pub width: u16,
    pub height: u16,
    pub home_sel: usize,
    pub browser: Browser,
    pub inspections: HashMap<PathBuf, Result<Inspection, String>>,
    pub result: Option<Outcome>,
    pub result_sel: usize,
    pub result_offset: usize,
    /// The search filter kept after `/` and Enter.
    pub filter: String,
    pub typing: Option<Typing>,
    pub modal: Option<Modal>,
    pub note: Option<Note>,
    pub job: Option<JobView>,
    pub home: Option<PathBuf>,
    flow: Option<Flow>,
    pending_g: bool,
    pending_select: Option<String>,
    pending_open: Option<PathBuf>,
    announce_dir: bool,
    key_typed: Option<String>,
    last_key: Option<String>,
    env_key: Option<String>,
    quit_after_job: bool,
    quit: bool,
}

impl App {
    /// A new interface whose file list starts in `dir`. `env_key` is the
    /// value of `FASTLOCK_KEY` (a path, not a secret).
    pub fn new(dir: PathBuf, home: Option<PathBuf>, env_key: Option<String>) -> Self {
        App {
            theme: Theme::default(),
            screen: Screen::Home,
            action: Action::Verify,
            width: 92,
            height: 26,
            home_sel: 0,
            browser: Browser {
                dir,
                items: Vec::new(),
                loaded: false,
                sel: 0,
                offset: 0,
            },
            inspections: HashMap::new(),
            result: None,
            result_sel: 0,
            result_offset: 0,
            filter: String::new(),
            typing: None,
            modal: None,
            note: None,
            job: None,
            home,
            flow: None,
            pending_g: false,
            pending_select: None,
            pending_open: None,
            announce_dir: false,
            key_typed: None,
            last_key: None,
            env_key,
            quit_after_job: false,
            quit: false,
        }
    }

    /// The effects to run once at start: list the first folder, selecting
    /// `select` in it when given.
    pub fn start(&mut self, select: Option<String>) -> Vec<Effect> {
        self.pending_select = select;
        vec![Effect::ListDir(self.browser.dir.clone())]
    }

    pub fn should_quit(&self) -> bool {
        self.quit
    }

    pub fn job_running(&self) -> bool {
        self.job.is_some()
    }

    /// Asks the running job to stop.
    pub fn cancel_job(&mut self) {
        if let Some(job) = &mut self.job {
            job.cancel.cancel();
            job.cancelling = true;
        }
    }

    pub fn geometry(&self) -> Geometry {
        Geometry::new(self.width, self.height)
    }

    // ---- what the screen shows ----

    /// The screen name in the top line.
    pub fn title(&self) -> String {
        match self.screen {
            Screen::Home => "home".to_string(),
            Screen::Files => self.action.label().to_string(),
            Screen::Result => match &self.result {
                Some(r) => format!("{} › {}", r.action.label(), file_name(&r.file)),
                None => self.action.label().to_string(),
            },
        }
    }

    /// The status at the right of the top line, and whether a job runs.
    pub fn status(&self) -> (String, bool) {
        match &self.job {
            Some(job) => {
                let text = match job.fraction() {
                    Some(f) => format!("{} {:.0}%", job.action.running(), f * 100.0),
                    None => format!("{}…", job.action.running()),
                };
                (text, true)
            }
            None => ("ready".to_string(), false),
        }
    }

    /// The key hints for the bottom line.
    pub fn hints(&self) -> &'static [(&'static str, &'static str)] {
        match self.screen {
            Screen::Home => &[
                ("j/k", "move"),
                ("Enter", "open"),
                ("/", "search"),
                (":", "command"),
                ("?", "help"),
                ("q", "quit"),
            ],
            Screen::Files => &[
                ("j/k", "move"),
                ("Enter", "open"),
                ("h", "parent"),
                ("/", "search"),
                ("Esc", "home"),
                ("?", "help"),
                ("q", "quit"),
            ],
            Screen::Result => &[
                ("j/k", "move"),
                ("h", "back"),
                ("/", "search"),
                ("?", "help"),
                ("q", "quit"),
            ],
        }
    }

    /// The text the list is filtered by, while typing or after Enter.
    pub fn needle(&self) -> String {
        match &self.typing {
            Some(t) if t.prompt == Prompt::Search => t.field.text().to_lowercase(),
            _ => self.filter.to_lowercase(),
        }
    }

    /// Indices into [`HOME_ITEMS`] that match the filter.
    pub fn visible_home(&self) -> Vec<usize> {
        let needle = self.needle();
        (0..HOME_ITEMS.len())
            .filter(|&i| HOME_ITEMS[i].label.contains(&needle))
            .collect()
    }

    /// Indices into the folder listing that match the filter.
    pub fn visible_files(&self) -> Vec<usize> {
        let needle = self.needle();
        (0..self.browser.items.len())
            .filter(|&i| self.browser.items[i].name.to_lowercase().contains(&needle))
            .collect()
    }

    /// Indices into the result entries that match the filter.
    pub fn visible_entries(&self) -> Vec<usize> {
        let needle = self.needle();
        match &self.result {
            Some(r) => (0..r.entries.len())
                .filter(|&i| r.entries[i].path.to_lowercase().contains(&needle))
                .collect(),
            None => Vec::new(),
        }
    }

    /// The lines of the home list: group headings, blanks between groups,
    /// and items.
    pub fn home_lines(&self) -> Vec<HomeLine> {
        let mut lines = Vec::new();
        let mut group = None;
        for (pos, &i) in self.visible_home().iter().enumerate() {
            let item = &HOME_ITEMS[i];
            if group != Some(item.group) {
                if group.is_some() {
                    lines.push(HomeLine::Blank);
                }
                lines.push(HomeLine::Heading(item.group));
                group = Some(item.group);
            }
            lines.push(HomeLine::Item(pos));
        }
        lines
    }

    pub fn selected_home(&self) -> Option<&'static HomeItem> {
        self.visible_home()
            .get(self.home_sel)
            .map(|&i| &HOME_ITEMS[i])
    }

    pub fn selected_item(&self) -> Option<&Item> {
        self.visible_files()
            .get(self.browser.sel)
            .map(|&i| &self.browser.items[i])
    }

    pub fn selected_entry(&self) -> Option<&fastlock_core::Entry> {
        let i = *self.visible_entries().get(self.result_sel)?;
        self.result.as_ref().map(|r| &r.entries[i])
    }

    fn list_len(&self) -> usize {
        match self.screen {
            Screen::Home => self.visible_home().len(),
            Screen::Files => self.visible_files().len(),
            Screen::Result => self.visible_entries().len(),
        }
    }

    fn sel(&self) -> usize {
        match self.screen {
            Screen::Home => self.home_sel,
            Screen::Files => self.browser.sel,
            Screen::Result => self.result_sel,
        }
    }

    fn set_sel(&mut self, sel: usize) {
        match self.screen {
            Screen::Home => self.home_sel = sel,
            Screen::Files => self.browser.sel = sel,
            Screen::Result => self.result_sel = sel,
        }
    }

    fn page(&self) -> usize {
        let g = self.geometry();
        match self.screen {
            Screen::Home => 1,
            Screen::Files => g.files_rows().max(1),
            Screen::Result => {
                let checks = self.result.as_ref().map_or(0, |r| r.checks.len());
                g.entries_rows(checks).max(1)
            }
        }
    }

    // ---- the update step ----

    /// Applies one event and returns what the caller must do.
    pub fn update(&mut self, event: Event) -> Vec<Effect> {
        let mut fx = Vec::new();
        match event {
            Event::Key(key) => self.on_key(key, &mut fx),
            Event::Click { column, row } => self.on_click(column, row, &mut fx),
            Event::Scroll(delta) => {
                if self.modal.is_none() && self.typing.is_none() {
                    self.move_by(if delta > 0 { 1 } else { -1 });
                }
            }
            Event::Resize(width, height) => {
                self.width = width;
                self.height = height;
            }
            Event::Paste(text) => self.on_paste(text),
            Event::Listed { dir, result } => self.on_listed(dir, result),
            Event::Inspected { path, result } => self.on_inspected(path, result, &mut fx),
            Event::KeyRead { path, result } => self.on_key_read(path, result, &mut fx),
            Event::Worker(message) => self.on_worker(message, &mut fx),
        }
        self.normalize();
        self.request_inspection(&mut fx);
        fx
    }

    fn on_key(&mut self, key: Key, fx: &mut Vec<Effect>) {
        if key == Key::Interrupt {
            if self.job.is_some() {
                self.cancel_job();
                self.quit_after_job = true;
            } else {
                self.do_quit(fx);
            }
            return;
        }
        if self.modal.is_some() {
            self.modal_key(key, fx);
        } else if self.typing.is_some() {
            self.typing_key(key, fx);
        } else {
            self.normal_key(key, fx);
        }
    }

    fn normal_key(&mut self, key: Key, fx: &mut Vec<Effect>) {
        let pending_g = std::mem::take(&mut self.pending_g);
        match key {
            Key::Char('j') | Key::Down => self.move_by(1),
            Key::Char('k') | Key::Up => self.move_by(-1),
            Key::PageDown => self.move_by(self.page() as isize),
            Key::PageUp => self.move_by(-(self.page() as isize)),
            Key::Char('g') => {
                if pending_g {
                    self.set_sel(0);
                } else {
                    self.pending_g = true;
                }
            }
            Key::Char('G') | Key::End => self.set_sel(self.list_len().saturating_sub(1)),
            Key::Home => self.set_sel(0),
            Key::Enter | Key::Char('l') | Key::Right => self.activate(fx),
            Key::Char('h') | Key::Left | Key::Backspace => self.back(fx),
            Key::Esc => {
                if self.filter.is_empty() {
                    self.escape(fx);
                } else {
                    self.filter.clear();
                }
            }
            Key::Char('/') => {
                self.filter.clear();
                self.typing = Some(Typing {
                    prompt: Prompt::Search,
                    field: TextField::default(),
                });
                self.set_sel(0);
            }
            Key::Char(':') => {
                self.typing = Some(Typing {
                    prompt: Prompt::Command,
                    field: TextField::default(),
                });
            }
            Key::Char('?') => self.modal = Some(Modal::Help),
            Key::Char('q') => self.do_quit(fx),
            Key::Char(c) if self.screen == Screen::Home => {
                if let Some(pos) = HOME_ITEMS.iter().position(|i| i.key == c) {
                    self.filter.clear();
                    self.home_sel = pos;
                    self.open_action(HOME_ITEMS[pos].action, fx);
                }
            }
            _ => {}
        }
    }

    fn typing_key(&mut self, key: Key, fx: &mut Vec<Effect>) {
        let Some(typing) = &mut self.typing else {
            return;
        };
        let prompt = typing.prompt;
        match key {
            Key::Enter => {
                let text = typing.field.text().to_string();
                self.typing = None;
                match prompt {
                    Prompt::Search => self.filter = text,
                    Prompt::Command => self.run_command(&text, fx),
                }
            }
            Key::Esc => {
                self.typing = None;
                if prompt == Prompt::Search {
                    self.filter.clear();
                }
            }
            Key::Backspace if typing.field.is_empty() => self.typing = None,
            Key::Up | Key::Down if prompt == Prompt::Search => {
                self.move_by(if key == Key::Down { 1 } else { -1 });
            }
            other => {
                edit_field(&mut typing.field, other);
                if prompt == Prompt::Search {
                    self.set_sel(0);
                }
            }
        }
    }

    fn modal_key(&mut self, key: Key, fx: &mut Vec<Effect>) {
        enum Next {
            Stay,
            Close,
            Output(String),
            KeyFile(String),
            Passphrase(SecretString),
            Cancel,
        }

        let is_output = matches!(self.modal, Some(Modal::Output { .. }));
        let next = match &mut self.modal {
            None => return,
            Some(Modal::Help) => Next::Close,
            Some(Modal::Progress) => {
                if key == Key::Esc {
                    Next::Cancel
                } else {
                    Next::Stay
                }
            }
            Some(Modal::Output { field, error }) | Some(Modal::KeyFile { field, error, .. }) => {
                match key {
                    Key::Esc => Next::Close,
                    Key::Enter => {
                        let text = field.text().trim().to_string();
                        if is_output {
                            Next::Output(text)
                        } else {
                            Next::KeyFile(text)
                        }
                    }
                    other => {
                        edit_field(field, other);
                        *error = None;
                        Next::Stay
                    }
                }
            }
            Some(Modal::Passphrase { field, error, .. }) => match key {
                Key::Esc => Next::Close,
                Key::Enter => {
                    if field.is_empty() {
                        *error = Some("empty passphrase".to_string());
                        Next::Stay
                    } else {
                        Next::Passphrase(field.take_secret())
                    }
                }
                other => {
                    edit_field(field, other);
                    *error = None;
                    Next::Stay
                }
            },
        };

        match next {
            Next::Stay => {}
            Next::Close => {
                if !matches!(self.modal, Some(Modal::Help)) {
                    self.flow = None;
                }
                self.modal = None;
            }
            Next::Cancel => self.cancel_job(),
            Next::Output(text) => self.submit_output(&text),
            Next::KeyFile(text) => self.submit_key_file(&text, fx),
            Next::Passphrase(secret) => self.submit_passphrase(secret, fx),
        }
    }

    fn on_click(&mut self, column: u16, row: u16, fx: &mut Vec<Effect>) {
        self.pending_g = false;
        if matches!(self.modal, Some(Modal::Help)) {
            self.modal = None;
            return;
        }
        if self.modal.is_some() {
            return;
        }
        if let Some(typing) = self.typing.take() {
            if typing.prompt == Prompt::Search {
                self.filter = typing.field.text().to_string();
            }
        }
        if let Some(index) = self.hit(column, row) {
            if index == self.sel() {
                self.activate(fx);
            } else {
                self.set_sel(index);
            }
        }
    }

    fn on_paste(&mut self, mut text: String) {
        if self.modal.is_some() {
            if let Some(Modal::Output { field, error })
            | Some(Modal::KeyFile { field, error, .. })
            | Some(Modal::Passphrase { field, error, .. }) = &mut self.modal
            {
                field.insert_str(&text);
                *error = None;
            }
        } else if let Some(typing) = &mut self.typing {
            typing.field.insert_str(&text);
            let search = typing.prompt == Prompt::Search;
            if search {
                self.set_sel(0);
            }
        }
        // A pasted passphrase must not stay behind in this buffer.
        text.zeroize();
    }

    /// The list index under a cell, if any.
    pub fn hit(&self, column: u16, row: u16) -> Option<usize> {
        let g = self.geometry();
        if column >= g.list_end() || row < g.body_y || row >= g.body_y + g.body_h {
            return None;
        }
        let (top, offset, rows) = match self.screen {
            Screen::Home => {
                let line = usize::from(row - g.body_y);
                return match self.home_lines().get(line) {
                    Some(HomeLine::Item(pos)) => Some(*pos),
                    _ => None,
                };
            }
            Screen::Files => (g.files_top(), self.browser.offset, g.files_rows()),
            Screen::Result => {
                let checks = self.result.as_ref().map_or(0, |r| r.checks.len());
                (
                    g.entries_top(checks),
                    self.result_offset,
                    g.entries_rows(checks),
                )
            }
        };
        if row < top {
            return None;
        }
        let line = usize::from(row - top);
        let index = offset + line;
        (line < rows && index < self.list_len()).then_some(index)
    }

    // ---- moving around ----

    fn move_by(&mut self, delta: isize) {
        let len = self.list_len();
        if len == 0 {
            return;
        }
        let sel = self.sel() as isize + delta;
        self.set_sel(sel.clamp(0, len as isize - 1) as usize);
    }

    fn go(&mut self, screen: Screen) {
        self.screen = screen;
        self.filter.clear();
        self.typing = None;
        self.note = None;
        self.pending_g = false;
    }

    fn open_action(&mut self, action: Action, fx: &mut Vec<Effect>) {
        self.action = action;
        self.go(Screen::Files);
        if !self.browser.loaded {
            fx.push(Effect::ListDir(self.browser.dir.clone()));
        }
    }

    fn activate(&mut self, fx: &mut Vec<Effect>) {
        match self.screen {
            Screen::Home => {
                if let Some(item) = self.selected_home() {
                    self.open_action(item.action, fx);
                }
            }
            Screen::Files => {
                let Some(item) = self.selected_item().cloned() else {
                    return;
                };
                if item.is_dir {
                    self.pending_select = None;
                    fx.push(Effect::ListDir(item.path));
                } else {
                    self.start_flow(item.path, fx);
                }
            }
            Screen::Result => {}
        }
    }

    fn back(&mut self, fx: &mut Vec<Effect>) {
        match self.screen {
            Screen::Home => {}
            Screen::Files => {
                if let Some(parent) = self.browser.dir.parent() {
                    self.pending_select = self
                        .browser
                        .dir
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned());
                    fx.push(Effect::ListDir(parent.to_path_buf()));
                }
            }
            Screen::Result => self.back_to_files(fx),
        }
    }

    fn escape(&mut self, fx: &mut Vec<Effect>) {
        match self.screen {
            Screen::Home => {}
            Screen::Files => self.go(Screen::Home),
            Screen::Result => self.back_to_files(fx),
        }
    }

    /// Returns to the file list and lists the folder again, so a file that
    /// was just written shows up.
    fn back_to_files(&mut self, fx: &mut Vec<Effect>) {
        self.pending_select = self.result.as_ref().map(|r| file_name(&r.file));
        self.go(Screen::Files);
        fx.push(Effect::ListDir(self.browser.dir.clone()));
    }

    fn do_quit(&mut self, fx: &mut Vec<Effect>) {
        self.quit = true;
        fx.push(Effect::Quit);
    }

    fn run_command(&mut self, text: &str, fx: &mut Vec<Effect>) {
        match parse_command(text) {
            Ok(None) => {}
            Ok(Some(Command::Quit)) => self.do_quit(fx),
            Ok(Some(Command::Help)) => self.modal = Some(Modal::Help),
            Ok(Some(Command::Open(action))) => {
                if let Some(pos) = HOME_ITEMS.iter().position(|i| i.action == action) {
                    self.home_sel = pos;
                }
                self.open_action(action, fx);
            }
            Ok(Some(Command::Cd(target))) => {
                let dir = match target {
                    Some(t) => resolve(&t, &self.browser.dir, self.home.as_deref()),
                    None => match &self.home {
                        Some(home) => home.clone(),
                        None => {
                            self.error("no home folder is known");
                            return;
                        }
                    },
                };
                self.pending_select = None;
                self.announce_dir = self.screen != Screen::Files;
                fx.push(Effect::ListDir(dir));
            }
            Err(message) => self.error(&message),
        }
    }

    // ---- results of effects ----

    fn on_listed(&mut self, dir: PathBuf, result: Result<Vec<Item>, String>) {
        let select = self.pending_select.take();
        let announce = std::mem::take(&mut self.announce_dir);
        match result {
            Ok(items) => {
                // A folder listed again may hold changed files.
                self.inspections
                    .retain(|path, _| path.parent() != Some(dir.as_path()));
                let sel = select
                    .and_then(|name| items.iter().position(|i| i.name == name))
                    .unwrap_or(0);
                self.browser = Browser {
                    dir,
                    items,
                    loaded: true,
                    sel,
                    offset: 0,
                };
                if self.screen == Screen::Files {
                    self.filter.clear();
                    if matches!(&self.typing, Some(t) if t.prompt == Prompt::Search) {
                        self.typing = None;
                    }
                }
                if announce {
                    let shown = display_path(&self.browser.dir, self.home.as_deref());
                    self.info(&format!("folder: {shown}"));
                }
            }
            Err(message) => self.error(&message),
        }
    }

    fn on_inspected(
        &mut self,
        path: PathBuf,
        result: Result<Inspection, String>,
        fx: &mut Vec<Effect>,
    ) {
        self.inspections.insert(path.clone(), result);
        if self.pending_open.as_ref() == Some(&path) {
            self.pending_open = None;
            self.start_flow(path, fx);
        }
    }

    fn request_inspection(&mut self, fx: &mut Vec<Effect>) {
        if self.screen != Screen::Files {
            return;
        }
        let Some(item) = self.selected_item() else {
            return;
        };
        if item.is_dir
            || self.inspections.contains_key(&item.path)
            || self.pending_open.as_ref() == Some(&item.path)
        {
            return;
        }
        fx.push(Effect::Inspect(item.path.clone()));
    }

    fn on_key_read(
        &mut self,
        _path: PathBuf,
        result: Result<IdentityFile, String>,
        fx: &mut Vec<Effect>,
    ) {
        if !matches!(self.modal, Some(Modal::KeyFile { .. })) || self.flow.is_none() {
            return;
        }
        match result {
            Ok(identity) => {
                self.last_key = self.key_typed.take();
                let identity = Arc::new(identity);
                if let Some(flow) = &mut self.flow {
                    flow.identity = Some(identity.clone());
                }
                if identity.needs_passphrase() {
                    self.modal = Some(Modal::Passphrase {
                        purpose: format!("Passphrase for key file {}", file_name(identity.path())),
                        field: TextField::secret(),
                        error: None,
                    });
                } else {
                    self.start_job(
                        Unlock::Key {
                            identity,
                            passphrase: None,
                        },
                        fx,
                    );
                }
            }
            Err(message) => {
                if let Some(Modal::KeyFile { error, .. }) = &mut self.modal {
                    *error = Some(message);
                }
            }
        }
    }

    fn on_worker(&mut self, message: Message, fx: &mut Vec<Effect>) {
        match message {
            Message::Unlocking => {
                if let Some(job) = &mut self.job {
                    job.unlocking = true;
                }
            }
            Message::Stage(stage) => {
                if let Some(job) = &mut self.job {
                    job.unlocking = false;
                    job.stage = Some(stage);
                    job.done = 0;
                    job.total = None;
                }
            }
            Message::Advance { done, total } => {
                if let Some(job) = &mut self.job {
                    job.done = done;
                    job.total = total;
                }
            }
            Message::Done(result) => {
                self.job = None;
                match result {
                    Ok(outcome) => self.show_outcome(outcome),
                    Err(err) => self.job_failed(&err),
                }
                if std::mem::take(&mut self.quit_after_job) {
                    self.modal = None;
                    self.do_quit(fx);
                }
            }
        }
    }

    // ---- opening a file ----

    fn start_flow(&mut self, path: PathBuf, fx: &mut Vec<Effect>) {
        let inspection = match self.inspections.get(&path) {
            None => {
                self.pending_open = Some(path.clone());
                fx.push(Effect::Inspect(path));
                return;
            }
            Some(Err(message)) => {
                let message = message.clone();
                self.error(&message);
                return;
            }
            Some(Ok(inspection)) => (inspection.format, inspection.needs),
        };
        let (format, needs) = inspection;
        let name = file_name(&path);
        if format == Format::Unknown {
            self.error(&format!("{name} is not an age file"));
            return;
        }
        if needs == Needs::Unknown {
            self.error(&format!("fastlock cannot tell how {name} is opened"));
            return;
        }
        self.note = None;
        self.flow = Some(Flow {
            action: self.action,
            path: path.clone(),
            needs,
            output: None,
            identity: None,
        });
        match self.action {
            Action::Decrypt => {
                let output = default_output(&path);
                self.modal = Some(Modal::Output {
                    field: TextField::new(&display_path(&output, self.home.as_deref())),
                    error: None,
                });
            }
            Action::Verify => self.ask_credentials(None),
        }
    }

    /// Shows the form for the key file or the passphrase.
    fn ask_credentials(&mut self, error: Option<String>) {
        let Some(flow) = &self.flow else {
            return;
        };
        let name = file_name(&flow.path);
        self.modal = Some(match flow.needs {
            Needs::Key => Modal::KeyFile {
                file: name,
                field: TextField::new(
                    self.last_key
                        .as_deref()
                        .or(self.env_key.as_deref())
                        .unwrap_or(""),
                ),
                error,
            },
            _ => Modal::Passphrase {
                purpose: format!("Passphrase for {name}"),
                field: TextField::secret(),
                error,
            },
        });
    }

    fn submit_output(&mut self, text: &str) {
        if text.is_empty() {
            if let Some(Modal::Output { error, .. }) = &mut self.modal {
                *error = Some("type where to write the decrypted data".to_string());
            }
            return;
        }
        let output = resolve(text, &self.browser.dir, self.home.as_deref());
        if let Some(flow) = &mut self.flow {
            flow.output = Some(output);
        }
        self.ask_credentials(None);
    }

    fn submit_key_file(&mut self, text: &str, fx: &mut Vec<Effect>) {
        if text.is_empty() {
            if let Some(Modal::KeyFile { error, .. }) = &mut self.modal {
                *error = Some("type the path of a key file".to_string());
            }
            return;
        }
        self.key_typed = Some(text.to_string());
        fx.push(Effect::ReadKey(resolve(
            text,
            &self.browser.dir,
            self.home.as_deref(),
        )));
    }

    fn submit_passphrase(&mut self, secret: SecretString, fx: &mut Vec<Effect>) {
        let Some(flow) = &self.flow else {
            return;
        };
        let unlock = match &flow.identity {
            Some(identity) => Unlock::Key {
                identity: identity.clone(),
                passphrase: Some(secret),
            },
            None => Unlock::Passphrase(secret),
        };
        self.start_job(unlock, fx);
    }

    fn start_job(&mut self, unlock: Unlock, fx: &mut Vec<Effect>) {
        let Some(flow) = &self.flow else {
            return;
        };
        let cancel = CancelToken::new();
        self.job = Some(JobView {
            action: flow.action,
            unlocking: false,
            stage: None,
            done: 0,
            total: None,
            cancelling: false,
            cancel: cancel.clone(),
        });
        self.modal = Some(Modal::Progress);
        self.note = None;
        fx.push(Effect::Start(Job {
            action: flow.action,
            path: flow.path.clone(),
            output: flow.output.clone(),
            unlock,
            cancel,
        }));
    }

    fn show_outcome(&mut self, outcome: Outcome) {
        self.modal = None;
        self.flow = None;
        let failed = outcome
            .checks
            .iter()
            .filter(|c| matches!(c.status, CheckStatus::Failed))
            .count();
        let verdict = if outcome.ok {
            match &outcome.output {
                Some(output) => Note {
                    text: format!(
                        "✓ decrypted to {} · every check passed",
                        display_path(output, self.home.as_deref())
                    ),
                    tone: Tone::Ok,
                },
                None => Note {
                    text: "✓ verified · every check passed · nothing was written".to_string(),
                    tone: Tone::Ok,
                },
            }
        } else {
            let checks = if failed == 1 { "check" } else { "checks" };
            Note {
                text: format!(
                    "✗ {} failed · {failed} {checks} did not pass",
                    outcome.action.label()
                ),
                tone: Tone::Error,
            }
        };
        self.result = Some(outcome);
        self.result_sel = 0;
        self.result_offset = 0;
        self.go(Screen::Result);
        self.note = Some(verdict);
    }

    fn job_failed(&mut self, err: &Error) {
        let message = err.message().to_string();
        let Some(flow) = &self.flow else {
            self.modal = None;
            self.error(&message);
            return;
        };
        let key_has_passphrase = flow
            .identity
            .as_ref()
            .is_some_and(|identity| identity.needs_passphrase());
        match err.kind() {
            ErrorKind::WrongKey if flow.needs == Needs::Passphrase || key_has_passphrase => {
                let purpose = match &flow.identity {
                    Some(identity) => {
                        format!("Passphrase for key file {}", file_name(identity.path()))
                    }
                    None => format!("Passphrase for {}", file_name(&flow.path)),
                };
                self.modal = Some(Modal::Passphrase {
                    purpose,
                    field: TextField::secret(),
                    error: Some(message),
                });
            }
            ErrorKind::WrongKey if flow.needs == Needs::Key => {
                self.ask_credentials(Some(message));
            }
            ErrorKind::Refused if flow.action == Action::Decrypt => {
                let shown = flow
                    .output
                    .as_deref()
                    .map(|o| display_path(o, self.home.as_deref()))
                    .unwrap_or_default();
                self.modal = Some(Modal::Output {
                    field: TextField::new(&shown),
                    error: Some(message),
                });
            }
            ErrorKind::Cancelled => {
                self.modal = None;
                self.flow = None;
                let text = match self.action {
                    Action::Verify => "cancelled · nothing was written",
                    Action::Decrypt => "cancelled",
                };
                self.note = Some(Note {
                    text: text.to_string(),
                    tone: Tone::Warning,
                });
            }
            _ => {
                self.modal = None;
                self.flow = None;
                self.error(&message);
            }
        }
    }

    // ---- helpers ----

    fn error(&mut self, text: &str) {
        self.note = Some(Note {
            text: text.to_string(),
            tone: Tone::Error,
        });
    }

    fn info(&mut self, text: &str) {
        self.note = Some(Note {
            text: text.to_string(),
            tone: Tone::Info,
        });
    }

    /// Keeps selections inside their lists and in view.
    fn normalize(&mut self) {
        let g = self.geometry();
        let homes = self.visible_home().len();
        self.home_sel = self.home_sel.min(homes.saturating_sub(1));

        let files = self.visible_files().len();
        self.browser.sel = self.browser.sel.min(files.saturating_sub(1));
        self.browser.offset =
            keep_visible(self.browser.sel, self.browser.offset, g.files_rows(), files);

        let entries = self.visible_entries().len();
        self.result_sel = self.result_sel.min(entries.saturating_sub(1));
        let checks = self.result.as_ref().map_or(0, |r| r.checks.len());
        self.result_offset = keep_visible(
            self.result_sel,
            self.result_offset,
            g.entries_rows(checks),
            entries,
        );
    }
}

/// Applies an editing key to a text field.
fn edit_field(field: &mut TextField, key: Key) {
    match key {
        Key::Char(c) => field.insert(c),
        Key::Backspace => {
            field.backspace();
        }
        Key::Delete => field.delete(),
        Key::Left => field.left(),
        Key::Right => field.right(),
        Key::Home => field.home(),
        Key::End => field.end(),
        Key::ClearLine => field.clear(),
        _ => {}
    }
}

/// A command typed after `:`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Quit,
    Help,
    Open(Action),
    /// `cd` with a path, or alone for the home folder.
    Cd(Option<String>),
}

/// Parses a command line. Empty input is no command.
pub fn parse_command(text: &str) -> Result<Option<Command>, String> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(None);
    }
    let (word, rest) = match text.split_once(char::is_whitespace) {
        Some((word, rest)) => (word, rest.trim()),
        None => (text, ""),
    };
    let command = match (word, rest.is_empty()) {
        ("q" | "quit", true) => Command::Quit,
        ("help", true) => Command::Help,
        ("verify", true) => Command::Open(Action::Verify),
        ("decrypt", true) => Command::Open(Action::Decrypt),
        ("cd", true) => Command::Cd(None),
        ("cd", false) => Command::Cd(Some(rest.to_string())),
        _ => return Err(format!("not a command: {text}")),
    };
    Ok(Some(command))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use fastlock_core::{Check, CheckId, Compression, Entry, EntryKind, Recipient, RecipientKind};
    use std::path::Path;

    pub(crate) fn item(name: &str, is_dir: bool, size: u64) -> Item {
        Item {
            name: name.to_string(),
            path: PathBuf::from("/sample/files").join(name),
            is_dir,
            size,
        }
    }

    pub(crate) fn inspection(path: &Path, needs: Needs) -> Inspection {
        let kind = match needs {
            Needs::Key => RecipientKind::X25519,
            _ => RecipientKind::Passphrase,
        };
        Inspection {
            path: path.to_path_buf(),
            format: Format::Age,
            size: 2048,
            armored: false,
            recipients: vec![Recipient {
                kind,
                stanza: "sample".to_string(),
            }],
            post_quantum: false,
            needs,
            header_size: Some(120),
            payload_size: Some(1928),
            checksum_file: None,
        }
    }

    pub(crate) fn outcome() -> Outcome {
        Outcome {
            action: Action::Verify,
            file: PathBuf::from("/sample/files/notes.tar.zst.age"),
            ok: true,
            checks: vec![
                Check {
                    id: CheckId::Decryption,
                    status: CheckStatus::Passed,
                    detail: "decrypted 1.2 MiB · stream complete".to_string(),
                },
                Check {
                    id: CheckId::ChecksumFile,
                    status: CheckStatus::Skipped,
                    detail: "no checksum file".to_string(),
                },
            ],
            compression: Compression::Zstd,
            archive: true,
            entries: vec![
                Entry {
                    path: "ideas.md".to_string(),
                    size: 14 * 1024,
                    kind: EntryKind::File,
                    sha256: Some("ab".repeat(32)),
                },
                Entry {
                    path: "scan-01.png".to_string(),
                    size: 1_100_000,
                    kind: EntryKind::File,
                    sha256: None,
                },
            ],
            output: None,
            skipped: Vec::new(),
            plaintext_size: 1_200_000,
        }
    }

    /// An app at 92 x 26 with a listed folder of sample files.
    pub(crate) fn sample_app() -> App {
        let mut app = App::new(
            PathBuf::from("/sample/files"),
            Some(PathBuf::from("/home/sample")),
            Some("~/keys/sample.txt".to_string()),
        );
        app.update(Event::Listed {
            dir: PathBuf::from("/sample/files"),
            result: Ok(vec![
                item("archive", true, 0),
                item("backup-2026-11.tar.zst.age", false, 8_800_000),
                item("notes.tar.zst.age", false, 1_250_000),
                item("readme.txt", false, 300),
            ]),
        });
        app
    }

    fn press(app: &mut App, keys: &str) -> Vec<Effect> {
        let mut fx = Vec::new();
        for c in keys.chars() {
            fx.extend(app.update(Event::Key(Key::Char(c))));
        }
        fx
    }

    fn key(app: &mut App, key: Key) -> Vec<Effect> {
        app.update(Event::Key(key))
    }

    fn note(app: &App) -> Option<(&str, Tone)> {
        app.note.as_ref().map(|n| (n.text.as_str(), n.tone))
    }

    /// Feeds every requested inspection back, as the event loop would.
    fn inspect_all(app: &mut App, fx: Vec<Effect>, needs: Needs) -> Vec<Effect> {
        let mut rest = Vec::new();
        for effect in fx {
            match effect {
                Effect::Inspect(path) => {
                    let result = Ok(inspection(&path, needs));
                    rest.extend(app.update(Event::Inspected { path, result }));
                }
                other => rest.push(other),
            }
        }
        rest
    }

    #[test]
    fn home_lists_only_what_works() {
        let app = sample_app();
        let labels: Vec<_> = HOME_ITEMS.iter().map(|i| i.label).collect();
        assert_eq!(labels, ["verify", "decrypt"]);
        assert_eq!(
            app.home_lines(),
            [
                HomeLine::Heading("Files"),
                HomeLine::Item(0),
                HomeLine::Item(1)
            ]
        );
    }

    #[test]
    fn shortcut_letters_do_not_collide_with_navigation_keys() {
        for item in &HOME_ITEMS {
            assert!(!"hjklgGq/:?".contains(item.key), "{}", item.key);
        }
    }

    #[test]
    fn j_and_k_move_and_stop_at_the_ends() {
        let mut app = sample_app();
        press(&mut app, "k");
        assert_eq!(app.home_sel, 0);
        press(&mut app, "jjj");
        assert_eq!(app.home_sel, 1);
        key(&mut app, Key::Up);
        assert_eq!(app.home_sel, 0);
    }

    #[test]
    fn gg_and_g_jump_to_the_first_and_last_file() {
        let mut app = sample_app();
        press(&mut app, "v");
        assert_eq!(app.screen, Screen::Files);
        press(&mut app, "G");
        assert_eq!(app.browser.sel, 3);
        press(&mut app, "g");
        assert_eq!(app.browser.sel, 3, "one g does nothing yet");
        press(&mut app, "g");
        assert_eq!(app.browser.sel, 0);
        press(&mut app, "gjg");
        assert_eq!(app.browser.sel, 1, "another key breaks gg");
    }

    #[test]
    fn a_shortcut_opens_the_file_list_for_its_action() {
        let mut app = sample_app();
        press(&mut app, "d");
        assert_eq!(app.screen, Screen::Files);
        assert_eq!(app.action, Action::Decrypt);
        assert_eq!(app.title(), "decrypt");
    }

    #[test]
    fn an_unlisted_folder_is_listed_when_the_file_list_opens() {
        let mut app = App::new(PathBuf::from("/sample"), None, None);
        let fx = press(&mut app, "v");
        assert!(matches!(&fx[..], [Effect::ListDir(dir)] if dir == Path::new("/sample")));
    }

    #[test]
    fn search_filters_while_typing_and_enter_keeps_it() {
        let mut app = sample_app();
        press(&mut app, "v/");
        press(&mut app, "notes");
        assert_eq!(app.visible_files().len(), 1);
        assert_eq!(app.selected_item().unwrap().name, "notes.tar.zst.age");
        key(&mut app, Key::Enter);
        assert!(app.typing.is_none());
        assert_eq!(app.filter, "notes");
        assert_eq!(app.visible_files().len(), 1);
        key(&mut app, Key::Esc);
        assert_eq!(app.filter, "");
        assert_eq!(app.visible_files().len(), 4);
        assert_eq!(
            app.screen,
            Screen::Files,
            "the first Esc only clears the filter"
        );
    }

    #[test]
    fn esc_while_searching_clears_the_filter() {
        let mut app = sample_app();
        press(&mut app, "v/zzz");
        assert_eq!(app.visible_files().len(), 0);
        key(&mut app, Key::Esc);
        assert!(app.typing.is_none());
        assert_eq!(app.visible_files().len(), 4);
    }

    #[test]
    fn h_goes_to_the_parent_folder_and_selects_where_it_came_from() {
        let mut app = sample_app();
        press(&mut app, "v");
        let fx = press(&mut app, "h");
        assert!(matches!(&fx[..], [Effect::ListDir(dir)] if dir == Path::new("/sample")));
        app.update(Event::Listed {
            dir: PathBuf::from("/sample"),
            result: Ok(vec![
                Item {
                    name: "alpha".to_string(),
                    path: PathBuf::from("/sample/alpha"),
                    is_dir: true,
                    size: 0,
                },
                Item {
                    name: "files".to_string(),
                    path: PathBuf::from("/sample/files"),
                    is_dir: true,
                    size: 0,
                },
            ]),
        });
        assert_eq!(app.browser.dir, PathBuf::from("/sample"));
        assert_eq!(app.selected_item().unwrap().name, "files");
    }

    #[test]
    fn enter_on_a_folder_lists_it() {
        let mut app = sample_app();
        press(&mut app, "v");
        let fx = key(&mut app, Key::Enter);
        assert!(
            matches!(&fx[..], [Effect::ListDir(dir)] if dir == Path::new("/sample/files/archive"))
        );
    }

    #[test]
    fn a_folder_that_cannot_be_listed_shows_a_red_message() {
        let mut app = sample_app();
        press(&mut app, "v");
        app.update(Event::Listed {
            dir: PathBuf::from("/sample/locked"),
            result: Err("cannot open the folder /sample/locked: permission denied".to_string()),
        });
        assert_eq!(app.browser.dir, PathBuf::from("/sample/files"));
        assert_eq!(note(&app).unwrap().1, Tone::Error);
    }

    #[test]
    fn esc_goes_back_and_q_quits() {
        let mut app = sample_app();
        press(&mut app, "v");
        key(&mut app, Key::Esc);
        assert_eq!(app.screen, Screen::Home);
        let fx = press(&mut app, "q");
        assert!(matches!(&fx[..], [Effect::Quit]));
        assert!(app.should_quit());
    }

    #[test]
    fn moving_onto_a_file_asks_for_its_inspection_once() {
        let mut app = sample_app();
        press(&mut app, "v");
        let fx = press(&mut app, "j");
        let fx = inspect_all(&mut app, fx, Needs::Passphrase);
        assert!(fx.is_empty());
        let path = app.selected_item().unwrap().path.clone();
        assert!(app.inspections.contains_key(&path));
        press(&mut app, "jk");
        let fx = press(&mut app, "k");
        assert!(fx.is_empty(), "folders and cached files need nothing");
    }

    #[test]
    fn the_command_line_understands_its_commands() {
        assert_eq!(parse_command("  "), Ok(None));
        assert_eq!(parse_command("q"), Ok(Some(Command::Quit)));
        assert_eq!(parse_command("quit"), Ok(Some(Command::Quit)));
        assert_eq!(parse_command("help"), Ok(Some(Command::Help)));
        assert_eq!(
            parse_command("verify"),
            Ok(Some(Command::Open(Action::Verify)))
        );
        assert_eq!(
            parse_command("decrypt"),
            Ok(Some(Command::Open(Action::Decrypt)))
        );
        assert_eq!(parse_command("cd"), Ok(Some(Command::Cd(None))));
        assert_eq!(
            parse_command("cd  ~/my files "),
            Ok(Some(Command::Cd(Some("~/my files".to_string()))))
        );
        assert_eq!(
            parse_command("encrypt"),
            Err("not a command: encrypt".to_string())
        );
        assert_eq!(
            parse_command("q now"),
            Err("not a command: q now".to_string())
        );
    }

    #[test]
    fn an_unknown_command_gives_a_red_line() {
        let mut app = sample_app();
        press(&mut app, ":open");
        key(&mut app, Key::Enter);
        assert_eq!(note(&app), Some(("not a command: open", Tone::Error)));
    }

    #[test]
    fn the_cd_command_lists_the_folder_and_says_so_from_home() {
        let mut app = sample_app();
        press(&mut app, ":cd ~/vault");
        let fx = key(&mut app, Key::Enter);
        assert!(
            matches!(&fx[..], [Effect::ListDir(dir)] if dir == Path::new("/home/sample/vault"))
        );
        app.update(Event::Listed {
            dir: PathBuf::from("/home/sample/vault"),
            result: Ok(Vec::new()),
        });
        assert_eq!(app.screen, Screen::Home);
        assert_eq!(note(&app).unwrap().1, Tone::Info);
    }

    #[test]
    fn help_opens_and_any_key_closes_it() {
        let mut app = sample_app();
        press(&mut app, "?");
        assert!(matches!(app.modal, Some(Modal::Help)));
        press(&mut app, "j");
        assert!(app.modal.is_none());
        assert_eq!(app.home_sel, 0, "the closing key does nothing else");
    }

    #[test]
    fn verify_with_a_passphrase_refuses_an_empty_one_then_starts() {
        let mut app = sample_app();
        press(&mut app, "v");
        let fx = press(&mut app, "j");
        inspect_all(&mut app, fx, Needs::Passphrase);
        key(&mut app, Key::Enter);
        assert!(matches!(app.modal, Some(Modal::Passphrase { .. })));

        key(&mut app, Key::Enter);
        assert!(matches!(
            &app.modal,
            Some(Modal::Passphrase { error: Some(e), .. }) if e == "empty passphrase"
        ));

        press(&mut app, "sample");
        let fx = key(&mut app, Key::Enter);
        assert!(matches!(&fx[..], [Effect::Start(job)] if job.action == Action::Verify));
        assert!(matches!(app.modal, Some(Modal::Progress)));
        assert!(app.job_running());
    }

    #[test]
    fn a_wrong_passphrase_returns_to_the_box_with_an_empty_field() {
        let mut app = sample_app();
        press(&mut app, "v");
        let fx = press(&mut app, "j");
        inspect_all(&mut app, fx, Needs::Passphrase);
        key(&mut app, Key::Enter);
        press(&mut app, "wrong");
        key(&mut app, Key::Enter);

        let err = Error::new(ErrorKind::WrongKey, "the passphrase is wrong");
        app.update(Event::Worker(Message::Done(Err(err))));
        match &app.modal {
            Some(Modal::Passphrase { field, error, .. }) => {
                assert!(field.is_empty());
                assert_eq!(error.as_deref(), Some("the passphrase is wrong"));
            }
            _ => panic!("the passphrase box is not shown"),
        }
        assert!(!app.job_running());
    }

    #[test]
    fn esc_in_the_progress_box_cancels_the_job() {
        let mut app = sample_app();
        press(&mut app, "v");
        let fx = press(&mut app, "j");
        inspect_all(&mut app, fx, Needs::Passphrase);
        key(&mut app, Key::Enter);
        press(&mut app, "sample");
        let fx = key(&mut app, Key::Enter);
        let Some(Effect::Start(job)) = fx.into_iter().next() else {
            panic!("no job was started");
        };
        key(&mut app, Key::Esc);
        assert!(job.cancel.is_cancelled());
        assert!(
            matches!(app.modal, Some(Modal::Progress)),
            "stays until the job ends"
        );

        let err = Error::new(ErrorKind::Cancelled, "cancelled");
        app.update(Event::Worker(Message::Done(Err(err))));
        assert!(app.modal.is_none());
        assert_eq!(note(&app).unwrap().1, Tone::Warning);
    }

    #[test]
    fn decrypt_asks_for_the_output_then_the_key_file() {
        let mut app = sample_app();
        press(&mut app, "d");
        let fx = press(&mut app, "j");
        inspect_all(&mut app, fx, Needs::Key);
        key(&mut app, Key::Enter);

        let expected = default_output(Path::new("/sample/files/backup-2026-11.tar.zst.age"));
        match &app.modal {
            Some(Modal::Output { field, .. }) => {
                assert_eq!(field.text(), expected.display().to_string());
            }
            _ => panic!("the output box is not shown"),
        }
        key(&mut app, Key::Enter);
        match &app.modal {
            Some(Modal::KeyFile { field, .. }) => assert_eq!(field.text(), "~/keys/sample.txt"),
            _ => panic!("the key file box is not shown"),
        }
        let fx = key(&mut app, Key::Enter);
        assert!(matches!(
            &fx[..],
            [Effect::ReadKey(path)] if path == Path::new("/home/sample/keys/sample.txt")
        ));

        app.update(Event::KeyRead {
            path: PathBuf::from("/home/sample/keys/sample.txt"),
            result: Err("cannot open the key file: not found".to_string()),
        });
        assert!(matches!(
            &app.modal,
            Some(Modal::KeyFile { error: Some(_), .. })
        ));
    }

    #[test]
    fn a_file_that_is_not_age_is_refused_plainly() {
        let mut app = sample_app();
        press(&mut app, "vG");
        let path = app.selected_item().unwrap().path.clone();
        let mut unknown = inspection(&path, Needs::Unknown);
        unknown.format = Format::Unknown;
        app.update(Event::Inspected {
            path,
            result: Ok(unknown),
        });
        key(&mut app, Key::Enter);
        assert!(app.modal.is_none());
        assert_eq!(
            note(&app),
            Some(("readme.txt is not an age file", Tone::Error))
        );
    }

    #[test]
    fn a_finished_job_shows_the_result_and_h_goes_back() {
        let mut app = sample_app();
        press(&mut app, "v");
        app.update(Event::Worker(Message::Done(Ok(outcome()))));
        assert_eq!(app.screen, Screen::Result);
        assert_eq!(note(&app).unwrap().1, Tone::Ok);
        press(&mut app, "j");
        assert_eq!(app.selected_entry().unwrap().path, "scan-01.png");
        let fx = press(&mut app, "h");
        assert_eq!(app.screen, Screen::Files);
        assert!(matches!(&fx[..], [Effect::ListDir(_)]));
    }

    #[test]
    fn clicking_selects_and_clicking_again_opens() {
        let mut app = sample_app();
        // Row 4 is the heading "Files", rows 5 and 6 are the items.
        app.update(Event::Click { column: 10, row: 6 });
        assert_eq!(app.home_sel, 1);
        assert_eq!(app.screen, Screen::Home);
        app.update(Event::Click { column: 10, row: 6 });
        assert_eq!(app.screen, Screen::Files);
        assert_eq!(app.action, Action::Decrypt);
        // A click in the preview pane selects nothing.
        app.update(Event::Click { column: 60, row: 7 });
        assert_eq!(app.browser.sel, 0);
    }

    #[test]
    fn the_wheel_moves_the_selection() {
        let mut app = sample_app();
        press(&mut app, "v");
        app.update(Event::Scroll(1));
        app.update(Event::Scroll(1));
        assert_eq!(app.browser.sel, 2);
        app.update(Event::Scroll(-1));
        assert_eq!(app.browser.sel, 1);
    }

    #[test]
    fn a_long_list_scrolls_with_the_selection() {
        let mut app = sample_app();
        let items = (0..50)
            .map(|i| item(&format!("file-{i:02}.age"), false, 1))
            .collect();
        app.update(Event::Listed {
            dir: PathBuf::from("/sample/files"),
            result: Ok(items),
        });
        press(&mut app, "vG");
        let rows = app.geometry().files_rows();
        assert_eq!(app.browser.offset, 50 - rows);
        let bottom = app.geometry().files_top() + rows as u16 - 1;
        assert_eq!(app.hit(5, bottom), Some(49));
    }

    #[test]
    fn ctrl_c_during_a_job_cancels_and_quits_when_it_ends() {
        let mut app = sample_app();
        press(&mut app, "v");
        let fx = press(&mut app, "j");
        inspect_all(&mut app, fx, Needs::Passphrase);
        key(&mut app, Key::Enter);
        press(&mut app, "sample");
        key(&mut app, Key::Enter);
        key(&mut app, Key::Interrupt);
        assert!(!app.should_quit());
        let err = Error::new(ErrorKind::Cancelled, "cancelled");
        let fx = app.update(Event::Worker(Message::Done(Err(err))));
        assert!(matches!(&fx[..], [Effect::Quit]));
        assert!(app.should_quit());
    }

    #[test]
    fn a_paste_goes_into_the_open_field() {
        let mut app = sample_app();
        press(&mut app, ":");
        app.update(Event::Paste("cd /srv/sample\n".to_string()));
        assert_eq!(app.typing.as_ref().unwrap().field.text(), "cd /srv/sample");
    }
}
