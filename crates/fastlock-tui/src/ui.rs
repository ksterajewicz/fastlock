//! Drawing. [`render`] draws the whole screen from the state and changes
//! nothing.
//!
//! Text is put straight into the frame's buffer, cell by cell, the same way
//! the approved mockup draws its grid.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::Span;
use ratatui::Frame;

use fastlock_core::{CheckStatus, Compression, EntryKind, Format, Inspection, Needs, Stage};

use crate::app::{App, HomeLine, Modal, Prompt, Screen, Tone, HOME_ITEMS};
use crate::field::TextField;
use crate::layout::{Geometry, LABEL_WIDTH, MARGIN};
use crate::listing::{display_path, file_name, Item};
use crate::theme::Theme;
use crate::worker::Action;

/// Draws the whole interface.
pub fn render(frame: &mut Frame<'_>, app: &App) {
    let area = frame.area();
    let mut c = Canvas {
        buf: frame.buffer_mut(),
        area,
    };
    let g = Geometry::new(area.width, area.height);
    let th = &app.theme;
    if g.too_small() {
        c.put(
            0,
            0,
            &fit_trim("fastlock: the terminal is too small", g.width),
            th.error,
        );
        return;
    }

    header(&mut c, &g, app);
    match app.screen {
        Screen::Home => draw_home(&mut c, &g, app),
        Screen::Files => draw_files(&mut c, &g, app),
        Screen::Result => draw_result(&mut c, &g, app),
    }
    if g.has_pane() {
        if let Some(x) = g.divider_x {
            for y in g.body_y..g.body_y + g.body_h {
                c.put(x, y, "│", th.secondary);
            }
        }
    }
    if let Some(note) = &app.note {
        let style = tone_style(th, note.tone);
        c.put(
            MARGIN,
            g.message_y,
            &fit_middle_trim(&note.text, g.width - 2 * MARGIN),
            style,
        );
    }
    footer(&mut c, &g, app);
    if let Some(modal) = &app.modal {
        draw_modal(&mut c, &g, app, modal);
    }
}

// ---- the cell canvas ----

struct Canvas<'a> {
    buf: &'a mut Buffer,
    area: Rect,
}

impl Canvas<'_> {
    /// Writes `text` at a cell and returns the column after it. Text that
    /// falls outside the screen is cut off.
    fn put(&mut self, x: u16, y: u16, text: &str, style: Style) -> u16 {
        let end = x.saturating_add(u16::try_from(width(text)).unwrap_or(u16::MAX));
        if y >= self.area.y && y < self.area.bottom() && x >= self.area.x && x < self.area.right() {
            self.buf.set_stringn(x, y, text, usize::MAX, style);
        }
        end
    }

    /// Blanks `w` cells from `x`, removing every style.
    fn blank(&mut self, x: u16, y: u16, w: u16) {
        self.put(x, y, &" ".repeat(usize::from(w)), Style::reset());
    }
}

/// The width of text in terminal cells.
pub(crate) fn width(text: &str) -> usize {
    Span::raw(text).width()
}

fn char_width(c: char) -> usize {
    let mut bytes = [0u8; 4];
    width(c.encode_utf8(&mut bytes))
}

/// Cuts text to `w` cells, ending with `…` when it was cut.
pub(crate) fn fit_trim(text: &str, w: u16) -> String {
    let w = usize::from(w);
    if width(text) <= w {
        return text.to_string();
    }
    let mut out = String::new();
    let mut used = 0;
    for c in text.chars() {
        let cw = char_width(c);
        if used + cw + 1 > w {
            break;
        }
        out.push(c);
        used += cw;
    }
    if w > 0 {
        out.push('…');
    }
    out
}

/// Cuts or pads text to exactly `w` cells.
pub(crate) fn fit(text: &str, w: u16) -> String {
    let mut out = fit_trim(text, w);
    let used = width(&out);
    out.extend(std::iter::repeat_n(
        ' ',
        usize::from(w).saturating_sub(used),
    ));
    out
}

/// Cuts text to `w` cells by removing its middle, so that both the start and
/// the end stay readable. Used for file names, where the ending tells files
/// apart.
pub(crate) fn fit_middle_trim(text: &str, w: u16) -> String {
    let cells = usize::from(w);
    if width(text) <= cells {
        return text.to_string();
    }
    if cells < 8 {
        return fit_trim(text, w);
    }
    let tail_w = (cells - 1) * 2 / 5;
    let head_w = cells - 1 - tail_w;
    let mut head = String::new();
    let mut used = 0;
    for c in text.chars() {
        let cw = char_width(c);
        if used + cw > head_w {
            break;
        }
        head.push(c);
        used += cw;
    }
    format!("{head}…{}", tail(text, tail_w))
}

/// Like [`fit_middle_trim`], padded to exactly `w` cells.
pub(crate) fn fit_middle(text: &str, w: u16) -> String {
    let mut out = fit_middle_trim(text, w);
    let used = width(&out);
    out.extend(std::iter::repeat_n(
        ' ',
        usize::from(w).saturating_sub(used),
    ));
    out
}

/// Cuts text to `w` cells by removing its start. Used for folder paths, where
/// the end says where you are.
pub(crate) fn fit_start_trim(text: &str, w: u16) -> String {
    let cells = usize::from(w);
    if width(text) <= cells {
        return text.to_string();
    }
    if cells == 0 {
        return String::new();
    }
    format!("…{}", tail(text, cells - 1))
}

/// Pads text on the left to `w` cells.
fn pad_left(text: &str, w: u16) -> String {
    let text = fit_trim(text, w);
    let pad = usize::from(w).saturating_sub(width(&text));
    format!("{}{text}", " ".repeat(pad))
}

/// Keeps the end of text when it is too long, for fields being typed in.
fn tail(text: &str, w: usize) -> String {
    let mut used = 0;
    let mut start = text.len();
    for (i, c) in text.char_indices().rev() {
        let cw = char_width(c);
        if used + cw > w {
            break;
        }
        used += cw;
        start = i;
    }
    text[start..].to_string()
}

/// Breaks text into lines of at most `w` cells at spaces.
pub(crate) fn wrap(text: &str, w: u16) -> Vec<String> {
    let w = usize::from(w.max(1));
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        let needed = if line.is_empty() {
            width(word)
        } else {
            width(&line) + 1 + width(word)
        };
        if needed > w && !line.is_empty() {
            lines.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines.into_iter().map(|l| fit_trim(&l, w as u16)).collect()
}

/// A size for people: `164 B`, `8.4 MiB`, `412 KiB`.
pub(crate) fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["KiB", "MiB", "GiB", "TiB", "PiB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64 / 1024.0;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if value < 10.0 {
        format!("{value:.1} {}", UNITS[unit])
    } else {
        format!("{value:.0} {}", UNITS[unit])
    }
}

fn tone_style(th: &Theme, tone: Tone) -> Style {
    match tone {
        Tone::Info => th.secondary,
        Tone::Ok => th.ok,
        Tone::Warning => th.warning,
        Tone::Error => th.error,
    }
}

// ---- shared pieces ----

fn header(c: &mut Canvas<'_>, g: &Geometry, app: &App) {
    let th = &app.theme;
    let (status, running) = app.status();
    let status_style = if running { th.accent } else { th.secondary };
    let status_x = g
        .width
        .saturating_sub(MARGIN)
        .saturating_sub(width(&status) as u16);
    let x = c.put(MARGIN, g.header_y, "fastlock", th.heading) + 2;
    let room = status_x.saturating_sub(x + 2);
    c.put(x, g.header_y, &fit_trim(&app.title(), room), th.secondary);
    c.put(status_x, g.header_y, &status, status_style);
    let rule = "─".repeat(usize::from(g.width.saturating_sub(2 * MARGIN)));
    c.put(MARGIN, g.rule_y, &rule, th.secondary);
}

fn footer(c: &mut Canvas<'_>, g: &Geometry, app: &App) {
    let th = &app.theme;
    let y = g.footer_y;
    let end = g.width.saturating_sub(MARGIN);
    if let Some(typing) = &app.typing {
        let prefix = match typing.prompt {
            Prompt::Search => "/",
            Prompt::Command => ":",
        };
        let x = c.put(MARGIN, y, prefix, th.text);
        draw_field(c, x, y, end.saturating_sub(x), &typing.field, th);
        return;
    }
    let mut x = MARGIN;
    if !app.filter.is_empty() {
        x = c.put(x, y, &fit_trim(&format!("/{}", app.filter), 24), th.accent) + 3;
    }
    for (key, label) in app.hints() {
        let needed = width(key) + 1 + width(label);
        if usize::from(x) + needed > usize::from(end) {
            break;
        }
        x = c.put(x, y, key, th.accent);
        x = c.put(x + 1, y, label, th.text) + 3;
    }
}

/// Draws a text field with its cursor. Secret fields show one `•` per
/// character.
fn draw_field(c: &mut Canvas<'_>, x: u16, y: u16, w: u16, field: &TextField, th: &Theme) {
    let w = usize::from(w.max(2));
    if field.is_secret() {
        let dots = field.char_count().min(w - 1);
        let x = c.put(x, y, &"•".repeat(dots), th.accent);
        c.put(x, y, "█", th.accent);
        return;
    }
    let text = field.text();
    let cursor = field.cursor();
    let before: String = text.chars().take(cursor).collect();
    let at = text.chars().nth(cursor);
    let after: String = text.chars().skip(cursor + 1).collect();
    // Keep the cursor in view: show the end of the text before it.
    let before = tail(&before, w - 1);
    let x = c.put(x, y, &before, th.text);
    match at {
        None => {
            c.put(x, y, "█", th.accent);
        }
        Some(ch) => {
            let mut bytes = [0u8; 4];
            let x = c.put(
                x,
                y,
                ch.encode_utf8(&mut bytes),
                th.accent.add_modifier(Modifier::REVERSED),
            );
            let room = w.saturating_sub(width(&before) + char_width(ch));
            c.put(x, y, &fit_trim(&after, room as u16), th.text);
        }
    }
}

/// One line of the preview pane.
enum PaneLine {
    Blank,
    Text(String, Style),
    /// A label and a value.
    Pair(String, String),
}

fn para(lines: &mut Vec<PaneLine>, text: &str, style: Style, w: u16) {
    for line in wrap(text, w) {
        lines.push(PaneLine::Text(line, style));
    }
}

fn draw_pane(c: &mut Canvas<'_>, g: &Geometry, th: &Theme, lines: &[PaneLine]) {
    if !g.has_pane() {
        return;
    }
    let label_w = LABEL_WIDTH.min(g.pane_w / 2);
    for (i, line) in lines.iter().take(usize::from(g.body_h)).enumerate() {
        let y = g.body_y + i as u16;
        match line {
            PaneLine::Blank => {}
            PaneLine::Text(text, style) => {
                c.put(g.pane_x, y, &fit_trim(text, g.pane_w), *style);
            }
            PaneLine::Pair(label, value) => {
                c.put(g.pane_x, y, &fit_trim(label, label_w), th.secondary);
                c.put(
                    g.pane_x + label_w,
                    y,
                    &fit_trim(value, g.pane_w - label_w),
                    th.text,
                );
            }
        }
    }
}

/// A list row: name on the left, a short value on the right. The selected
/// row is one continuous bar.
fn list_row(
    c: &mut Canvas<'_>,
    g: &Geometry,
    th: &Theme,
    y: u16,
    selected: bool,
    name: &str,
    extra: &str,
) {
    let extra_w = 11.min(g.list_w / 3);
    let name_w = g.list_w.saturating_sub(extra_w + 2);
    if selected {
        let bar = format!(" {}{} ", fit_middle(name, name_w), pad_left(extra, extra_w));
        c.put(g.list_x, y, &bar, th.selection);
    } else {
        c.put(g.list_x + 1, y, &fit_middle(name, name_w), th.text);
        c.put(
            g.list_x + 1 + name_w,
            y,
            &pad_left(extra, extra_w),
            th.secondary,
        );
    }
}

// ---- screens ----

fn draw_home(c: &mut Canvas<'_>, g: &Geometry, app: &App) {
    let th = &app.theme;
    let visible = app.visible_home();
    if visible.is_empty() {
        c.put(g.list_x + 1, g.body_y, "no match", th.secondary);
    }
    let label_w: u16 = 12;
    let state_w = g.list_w.saturating_sub(label_w + 5);
    for (i, line) in app.home_lines().iter().enumerate() {
        let y = g.body_y + i as u16;
        if i >= usize::from(g.body_h) {
            break;
        }
        match line {
            HomeLine::Heading(group) => {
                c.put(g.list_x, y, group, th.heading);
            }
            HomeLine::Blank => {}
            HomeLine::Item(pos) => {
                let item = &HOME_ITEMS[visible[*pos]];
                if *pos == app.home_sel {
                    let bar = format!(
                        " {}  {}{} ",
                        item.key,
                        fit(item.label, label_w),
                        fit(item.state, state_w)
                    );
                    c.put(g.list_x, y, &bar, th.selection);
                } else {
                    let mut bytes = [0u8; 4];
                    c.put(g.list_x + 1, y, item.key.encode_utf8(&mut bytes), th.accent);
                    c.put(g.list_x + 4, y, &fit(item.label, label_w), th.text);
                    c.put(
                        g.list_x + 4 + label_w,
                        y,
                        &fit(item.state, state_w),
                        th.secondary,
                    );
                }
            }
        }
    }

    let Some(item) = app.selected_home() else {
        return;
    };
    let w = g.pane_w;
    let mut lines = vec![
        PaneLine::Text(item.label.to_string(), th.heading),
        PaneLine::Blank,
    ];
    match item.action {
        Action::Verify => {
            para(
                &mut lines,
                "Decrypts in memory and checks every checksum. Writes nothing to disk.",
                th.text,
                w,
            );
            lines.push(PaneLine::Blank);
            lines.push(PaneLine::Text("Result".to_string(), th.secondary));
            para(
                &mut lines,
                "green only after the last byte has been authenticated",
                th.text,
                w,
            );
        }
        Action::Decrypt => {
            para(
                &mut lines,
                "Decrypts to disk and unpacks an archive. Asks where to write first.",
                th.text,
                w,
            );
            lines.push(PaneLine::Blank);
            lines.push(PaneLine::Text("Result".to_string(), th.secondary));
            para(
                &mut lines,
                "reported only after the last byte has been authenticated",
                th.text,
                w,
            );
        }
    }
    draw_pane(c, g, th, &lines);
}

fn draw_files(c: &mut Canvas<'_>, g: &Geometry, app: &App) {
    let th = &app.theme;
    let dir = display_path(&app.browser.dir, app.home.as_deref());
    c.put(
        g.list_x,
        g.body_y,
        &fit_start_trim(&dir, g.list_w),
        th.secondary,
    );

    let visible = app.visible_files();
    let top = g.files_top();
    if visible.is_empty() {
        let text = if !app.browser.loaded {
            "reading…"
        } else if app.browser.items.is_empty() {
            "empty folder"
        } else {
            "no match"
        };
        c.put(g.list_x + 1, top, text, th.secondary);
    }
    for row in 0..g.files_rows() {
        let pos = app.browser.offset + row;
        let Some(&index) = visible.get(pos) else {
            break;
        };
        let item = &app.browser.items[index];
        let extra = if item.is_dir {
            String::new()
        } else {
            human_size(item.size)
        };
        let y = top + row as u16;
        list_row(c, g, th, y, pos == app.browser.sel, &item.label(), &extra);
    }

    if let Some(item) = app.selected_item() {
        let lines = file_pane(app, item, g.pane_w);
        draw_pane(c, g, th, &lines);
    }
}

fn file_pane(app: &App, item: &Item, w: u16) -> Vec<PaneLine> {
    let th = &app.theme;
    let mut lines = vec![
        PaneLine::Text(fit_middle_trim(&item.label(), w), th.heading),
        PaneLine::Blank,
    ];
    if item.is_dir {
        lines.push(PaneLine::Text("folder".to_string(), th.secondary));
        lines.push(PaneLine::Text("Enter opens it.".to_string(), th.secondary));
        return lines;
    }
    match app.inspections.get(&item.path) {
        None => lines.push(PaneLine::Text("reading…".to_string(), th.secondary)),
        Some(Err(message)) => para(&mut lines, message, th.error, w),
        Some(Ok(inspection)) if inspection.format == Format::Unknown => {
            lines.push(PaneLine::Text("not an age file".to_string(), th.text));
            para(&mut lines, "fastlock cannot open it.", th.secondary, w);
            lines.push(PaneLine::Blank);
            lines.push(PaneLine::Pair(
                "size".to_string(),
                human_size(inspection.size),
            ));
        }
        Some(Ok(inspection)) => inspection_lines(app, inspection, &mut lines, w),
    }
    lines
}

fn inspection_lines(app: &App, ins: &Inspection, lines: &mut Vec<PaneLine>, w: u16) {
    let th = &app.theme;
    let mut format = ins.format.label().to_string();
    if ins.armored {
        format.push_str(" · armored");
    }
    lines.push(PaneLine::Pair("format".to_string(), format));

    // Who can open it: each kind of recipient once, with a count.
    let mut kinds: Vec<(&'static str, usize)> = Vec::new();
    for recipient in &ins.recipients {
        let label = recipient.kind.label();
        match kinds.iter_mut().find(|(l, _)| *l == label) {
            Some((_, n)) => *n += 1,
            None => kinds.push((label, 1)),
        }
    }
    if kinds.is_empty() {
        lines.push(PaneLine::Pair(
            "opens with".to_string(),
            "nobody listed".to_string(),
        ));
    }
    for (i, (label, n)) in kinds.iter().take(3).enumerate() {
        let value = if *n > 1 {
            format!("{n} × {label}")
        } else {
            (*label).to_string()
        };
        let name = if i == 0 { "opens with" } else { "" };
        lines.push(PaneLine::Pair(name.to_string(), value));
    }
    if kinds.len() > 3 {
        lines.push(PaneLine::Pair(
            String::new(),
            format!("and {} more", kinds.len() - 3),
        ));
    }

    let pq = if ins.post_quantum { "yes" } else { "no" };
    lines.push(PaneLine::Pair("post-quantum".to_string(), pq.to_string()));
    lines.push(PaneLine::Pair("size".to_string(), human_size(ins.size)));
    let checksum = match &ins.checksum_file {
        Some(path) => format!("yes · {}", file_name(path)),
        None => "no".to_string(),
    };
    lines.push(PaneLine::Pair("checksum".to_string(), checksum));
    lines.push(PaneLine::Blank);

    let asks = match ins.needs {
        Needs::Key => "Enter asks for the key file.",
        Needs::Passphrase => "Enter asks for the passphrase.",
        Needs::Unknown => "fastlock cannot tell how it is opened.",
    };
    let then = match app.action {
        Action::Verify => "Nothing is written to disk.",
        Action::Decrypt => "Then it is decrypted to disk.",
    };
    para(lines, &format!("{asks} {then}"), th.secondary, w);
}

fn draw_result(c: &mut Canvas<'_>, g: &Geometry, app: &App) {
    let th = &app.theme;
    let Some(result) = &app.result else {
        return;
    };
    let shown = g.checks_shown(result.checks.len());
    for (i, check) in result.checks.iter().take(shown).enumerate() {
        let y = g.body_y + i as u16;
        #[allow(unreachable_patterns)]
        let (mark, style) = match check.status {
            CheckStatus::Passed => ("✓", th.ok),
            CheckStatus::Failed => ("✗", th.error),
            CheckStatus::Skipped => ("–", th.secondary),
            _ => ("?", th.secondary),
        };
        let text = if check.detail.is_empty() {
            check.id.title().to_string()
        } else {
            check.detail.clone()
        };
        c.put(g.list_x + 1, y, mark, style);
        c.put(
            g.list_x + 3,
            y,
            &fit_middle_trim(&text, g.list_w.saturating_sub(3)),
            th.text,
        );
    }

    let top = g.entries_top(result.checks.len());
    c.put(g.list_x, top - 1, "Contents", th.heading);
    let visible = app.visible_entries();
    if visible.is_empty() {
        let text = if result.entries.is_empty() {
            "nothing listed"
        } else {
            "no match"
        };
        c.put(g.list_x + 1, top, text, th.secondary);
    }
    for row in 0..g.entries_rows(result.checks.len()) {
        let pos = app.result_offset + row;
        let Some(&index) = visible.get(pos) else {
            break;
        };
        let entry = &result.entries[index];
        #[allow(unreachable_patterns)]
        let (name, extra) = match entry.kind {
            EntryKind::Directory => (format!("{}/", entry.path), String::new()),
            EntryKind::File => (entry.path.clone(), human_size(entry.size)),
            _ => (entry.path.clone(), String::new()),
        };
        let y = top + row as u16;
        list_row(c, g, th, y, pos == app.result_sel, &name, &extra);
    }

    let w = g.pane_w;
    let mut lines = Vec::new();
    match app.selected_entry() {
        Some(entry) => {
            lines.push(PaneLine::Text(entry.path.clone(), th.heading));
            lines.push(PaneLine::Blank);
            lines.push(PaneLine::Pair("size".to_string(), human_size(entry.size)));
            #[allow(unreachable_patterns)]
            let kind = match entry.kind {
                EntryKind::File => "file",
                EntryKind::Directory => "folder",
                EntryKind::Other => "other",
                _ => "other",
            };
            lines.push(PaneLine::Pair("type".to_string(), kind.to_string()));
            if let Some(sha) = &entry.sha256 {
                lines.push(PaneLine::Text("SHA-256".to_string(), th.secondary));
                let per_line = usize::from(w.clamp(1, 32));
                let chars: Vec<char> = sha.chars().collect();
                for chunk in chars.chunks(per_line) {
                    lines.push(PaneLine::Text(chunk.iter().collect(), th.text));
                }
            }
        }
        None => {
            lines.push(PaneLine::Text(file_name(&result.file), th.heading));
            lines.push(PaneLine::Blank);
            lines.push(PaneLine::Pair(
                "decrypted".to_string(),
                human_size(result.plaintext_size),
            ));
            #[allow(unreachable_patterns)]
            let compression = match result.compression {
                Compression::None => "none",
                Compression::Zstd => "zstd",
                _ => "other",
            };
            lines.push(PaneLine::Pair(
                "compression".to_string(),
                compression.to_string(),
            ));
            let archive = if result.archive { "yes" } else { "no" };
            lines.push(PaneLine::Pair("archive".to_string(), archive.to_string()));
        }
    }
    lines.push(PaneLine::Blank);
    match &result.output {
        Some(output) => {
            lines.push(PaneLine::Text("Written to".to_string(), th.secondary));
            para(
                &mut lines,
                &display_path(output, app.home.as_deref()),
                th.text,
                w,
            );
        }
        None => {
            lines.push(PaneLine::Text("Verify".to_string(), th.secondary));
            para(
                &mut lines,
                "checked in memory; the file on disk is unchanged",
                th.text,
                w,
            );
        }
    }
    if !result.skipped.is_empty() {
        lines.push(PaneLine::Blank);
        let n = result.skipped.len();
        let entries = if n == 1 { "entry" } else { "entries" };
        para(
            &mut lines,
            &format!("! {n} {entries} skipped"),
            th.warning,
            w,
        );
    }
    draw_pane(c, g, th, &lines);
}

// ---- boxes ----

/// Clears a centered box, draws its rounded border with the title, and
/// returns its top-left cell and its size.
fn draw_box(
    c: &mut Canvas<'_>,
    g: &Geometry,
    th: &Theme,
    w: u16,
    h: u16,
    title: &str,
) -> (u16, u16, u16, u16) {
    let w = w.min(g.width.saturating_sub(4)).max(12);
    let h = h.min(g.height);
    let y0 = (g.height - h) / 2;
    let x0 = (g.width.saturating_sub(w)) / 2;
    for y in y0..y0 + h {
        c.blank(x0.saturating_sub(2), y, w + 4);
    }
    let title = fit_trim(title, w.saturating_sub(6));
    let title_w = width(&title) as u16;
    let top = format!(
        "╭─ {} {}╮",
        " ".repeat(usize::from(title_w)),
        "─".repeat(usize::from(w.saturating_sub(5 + title_w)))
    );
    c.put(x0, y0, &top, th.secondary);
    c.put(x0 + 3, y0, &title, th.heading);
    for y in y0 + 1..y0 + h - 1 {
        c.put(x0, y, "│", th.secondary);
        c.put(x0 + w - 1, y, "│", th.secondary);
    }
    let bottom = format!("╰{}╯", "─".repeat(usize::from(w - 2)));
    c.put(x0, y0 + h - 1, &bottom, th.secondary);
    (x0, y0, w, h)
}

fn hints_at(c: &mut Canvas<'_>, th: &Theme, x: u16, y: u16, hints: &[(&str, &str)]) {
    let mut x = x;
    for (key, label) in hints {
        x = c.put(x, y, key, th.accent);
        x = c.put(x + 1, y, label, th.text) + 3;
    }
}

fn draw_modal(c: &mut Canvas<'_>, g: &Geometry, app: &App, modal: &Modal) {
    let th = &app.theme;
    match modal {
        Modal::Help => draw_help(c, g, th),
        Modal::Progress => draw_progress(c, g, app),
        Modal::Output { field, error } => {
            let (x0, y0, w, _) = draw_box(c, g, th, 56, 8, "decrypt to");
            let x = x0 + 3;
            let inner = w.saturating_sub(6);
            c.put(
                x,
                y0 + 2,
                &fit_trim("Write the decrypted data to", inner),
                th.text,
            );
            draw_field(c, x, y0 + 3, inner, field, th);
            if let Some(e) = error {
                c.put(x, y0 + 4, &fit_middle_trim(e, inner), th.error);
            }
            hints_at(c, th, x, y0 + 6, &[("Enter", "next"), ("Esc", "cancel")]);
        }
        Modal::KeyFile { file, field, error } => {
            let (x0, y0, w, _) = draw_box(c, g, th, 56, 8, "key file");
            let x = x0 + 3;
            let inner = w.saturating_sub(6);
            c.put(
                x,
                y0 + 2,
                &fit_trim(&format!("Key file for {file}"), inner),
                th.text,
            );
            draw_field(c, x, y0 + 3, inner, field, th);
            match error {
                Some(e) => c.put(x, y0 + 4, &fit_middle_trim(e, inner), th.error),
                None => c.put(
                    x,
                    y0 + 4,
                    &fit_trim("~ is your home folder", inner),
                    th.secondary,
                ),
            };
            hints_at(c, th, x, y0 + 6, &[("Enter", "next"), ("Esc", "cancel")]);
        }
        Modal::Passphrase {
            purpose,
            field,
            error,
        } => {
            let (x0, y0, w, _) = draw_box(c, g, th, 52, 8, "unlock");
            let x = x0 + 3;
            let inner = w.saturating_sub(6);
            c.put(x, y0 + 2, &fit_trim(purpose, inner), th.text);
            draw_field(c, x, y0 + 3, inner, field, th);
            if let Some(e) = error {
                c.put(x, y0 + 4, &fit_middle_trim(e, inner), th.error);
            }
            hints_at(c, th, x, y0 + 6, &[("Enter", "unlock"), ("Esc", "cancel")]);
        }
    }
}

fn draw_progress(c: &mut Canvas<'_>, g: &Geometry, app: &App) {
    let th = &app.theme;
    let Some(job) = &app.job else {
        return;
    };
    let (x0, y0, w, _) = draw_box(c, g, th, 52, 7, job.action.label());
    let x = x0 + 3;
    let bar_w = w.saturating_sub(14).max(4);
    match job.fraction() {
        Some(f) => {
            let filled = ((f * f64::from(bar_w)).round() as u16).min(bar_w);
            let after = c.put(x, y0 + 2, &"█".repeat(usize::from(filled)), th.accent);
            c.put(
                after,
                y0 + 2,
                &"░".repeat(usize::from(bar_w - filled)),
                th.secondary,
            );
            let pct = format!("{:>3.0}%", f * 100.0);
            c.put(x + bar_w + 1, y0 + 2, &pct, th.text);
        }
        None => {
            c.put(x, y0 + 2, &"░".repeat(usize::from(bar_w)), th.secondary);
            if job.done > 0 {
                c.put(x + bar_w + 1, y0 + 2, &human_size(job.done), th.text);
            }
        }
    }
    let stage = if job.cancelling {
        "cancelling…".to_string()
    } else if job.unlocking {
        "unlocking the key".to_string()
    } else {
        match job.stage {
            None => "starting".to_string(),
            Some(Stage::Checksum) => "checking the checksum file".to_string(),
            Some(Stage::Decrypt) => match job.total {
                Some(total) => format!("decrypting · {}", human_size(total)),
                None => "decrypting".to_string(),
            },
            Some(Stage::Finish) => "finishing".to_string(),
        }
    };
    c.put(
        x,
        y0 + 3,
        &fit_trim(&stage, w.saturating_sub(6)),
        th.secondary,
    );
    hints_at(c, th, x, y0 + 5, &[("Esc", "cancel")]);
}

const HELP_ROWS: [(&str, &str); 11] = [
    ("j k ↑ ↓", "move"),
    ("h l ← →", "back · open"),
    ("gg  G", "first · last"),
    ("Enter", "open"),
    ("/", "search this list"),
    (":", "command  :verify :decrypt :cd :q"),
    ("v d", "jump straight to an item (home)"),
    ("Esc", "back · cancel"),
    ("Ctrl+U", "clear a text field"),
    ("q", "quit"),
    ("mouse", "click selects · again opens · wheel moves"),
];

fn draw_help(c: &mut Canvas<'_>, g: &Geometry, th: &Theme) {
    let h = HELP_ROWS.len() as u16 + 5;
    let (x0, y0, w, h) = draw_box(c, g, th, 62, h, "keys");
    let x = x0 + 3;
    let text_w = w.saturating_sub(6 + 14);
    for (i, (key, label)) in HELP_ROWS.iter().enumerate() {
        let y = y0 + 2 + i as u16;
        if y + 2 >= y0 + h {
            break;
        }
        c.put(x, y, key, th.accent);
        c.put(x + 14, y, &fit_trim(label, text_w), th.text);
    }
    c.put(x, y0 + h - 2, "any key closes this", th.secondary);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::tests::{inspection, outcome, sample_app};
    use crate::app::{Event, Key};
    use crate::worker::Message;
    use ratatui::backend::TestBackend;
    use ratatui::style::Color;
    use ratatui::Terminal;

    fn draw(app: &App, width: u16, height: u16) -> Buffer {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|frame| render(frame, app)).unwrap();
        terminal.backend().buffer().clone()
    }

    fn row(buf: &Buffer, y: u16) -> String {
        (0..buf.area().width)
            .map(|x| buf[(x, y)].symbol().to_string())
            .collect()
    }

    fn sized(width: u16, height: u16) -> App {
        let mut app = sample_app();
        app.update(Event::Resize(width, height));
        app
    }

    #[test]
    fn home_at_92_by_26_matches_the_mockup() {
        let app = sized(92, 26);
        let buf = draw(&app, 92, 26);
        assert_eq!(row(&buf, 0).trim(), "");
        let header = row(&buf, 1);
        assert!(header.starts_with("  fastlock  home"), "{header}");
        assert!(header.trim_end().ends_with("ready"), "{header}");
        assert!(buf[(2, 1)].modifier.contains(Modifier::BOLD));
        assert!(row(&buf, 2).trim().chars().all(|c| c == '─'));
        assert!(row(&buf, 4).starts_with("  Files "));
        assert!(row(&buf, 25).starts_with("  j/k move   Enter open   / search"));

        let selected = row(&buf, 5);
        assert!(
            selected.starts_with("   v  verify      check, write nothing"),
            "{selected}"
        );
        for x in 2..45 {
            assert_eq!(buf[(x, 5)].bg, Color::Blue, "column {x}");
        }
        assert_eq!(buf[(45, 5)].bg, Color::Reset);
        assert_eq!(
            buf[(3, 6)].fg,
            Color::LightBlue,
            "shortcut letter in the accent color"
        );
        assert_eq!(buf[(48, 10)].symbol(), "│");
        assert!(
            row(&buf, 4).contains("verify"),
            "the preview names the action"
        );
    }

    #[test]
    fn home_at_80_by_24_keeps_everything_in_view() {
        let app = sized(80, 24);
        let buf = draw(&app, 80, 24);
        assert!(row(&buf, 1).starts_with("  fastlock  home"));
        assert!(row(&buf, 1).trim_end().ends_with("ready"));
        let footer = row(&buf, 23);
        assert!(footer.starts_with("  j/k move"), "{footer}");
        assert!(footer.contains("q quit"), "{footer}");
        assert_eq!(buf[(2, 5)].bg, Color::Blue);
        assert_eq!(buf[(42, 10)].symbol(), "│");
    }

    #[test]
    fn the_file_list_shows_folders_first_and_the_selection() {
        let mut app = sized(80, 24);
        app.update(Event::Key(Key::Char('v')));
        app.update(Event::Key(Key::Char('j')));
        let path = app.selected_item().unwrap().path.clone();
        let result = Ok(inspection(&path, Needs::Passphrase));
        app.update(Event::Inspected { path, result });
        let buf = draw(&app, 80, 24);
        assert!(row(&buf, 1).starts_with("  fastlock  verify"));
        assert!(row(&buf, 4).starts_with("  /sample/files"));
        assert!(row(&buf, 5).starts_with("   archive/"));
        // At 80 columns the list is narrower, so a long name loses its
        // middle. Its start and its ending must still be there.
        let selected = row(&buf, 6);
        assert!(selected.starts_with("   backup-2026-11"), "{selected}");
        assert!(selected.contains(".age"), "{selected}");
        assert_eq!(buf[(2, 6)].bg, Color::Blue);
        assert!(
            row(&buf, 4).contains("backup-2026-11"),
            "the preview names the file"
        );
        assert!(
            row(&buf, 6).contains("age v1"),
            "the preview shows the format"
        );
        assert!(row(&buf, 23).contains("h parent"));
    }

    #[test]
    fn a_narrow_terminal_drops_the_preview() {
        let mut app = sized(60, 24);
        app.update(Event::Key(Key::Char('v')));
        let buf = draw(&app, 60, 24);
        for y in 4..22 {
            assert!(!row(&buf, y).contains('│'), "row {y}");
        }
        assert_eq!(buf[(2, 5)].bg, Color::Blue);
        assert_eq!(
            buf[(57, 5)].bg,
            Color::Blue,
            "the bar spans the whole width"
        );
    }

    #[test]
    fn the_passphrase_box_shows_dots_only() {
        let mut app = sized(92, 26);
        app.update(Event::Key(Key::Char('v')));
        app.update(Event::Key(Key::Char('j')));
        let path = app.selected_item().unwrap().path.clone();
        let result = Ok(inspection(&path, Needs::Passphrase));
        app.update(Event::Inspected { path, result });
        app.update(Event::Key(Key::Enter));
        for c in "secret".chars() {
            app.update(Event::Key(Key::Char(c)));
        }
        let buf = draw(&app, 92, 26);
        let all: String = (0..26).map(|y| row(&buf, y)).collect();
        assert!(all.contains("╭─ unlock "));
        assert!(all.contains("••••••█"));
        assert!(!all.contains("secret"));
        assert!(all.contains("Enter unlock   Esc cancel"));
    }

    #[test]
    fn the_result_lists_checks_and_contents() {
        let mut app = sized(92, 26);
        app.update(Event::Worker(Message::Done(Ok(outcome()))));
        let buf = draw(&app, 92, 26);
        assert!(row(&buf, 4).starts_with("   ✓ decrypted 1.2 MiB"));
        assert_eq!(buf[(3, 4)].fg, Color::LightGreen);
        assert!(row(&buf, 5).starts_with("   – no checksum file"));
        assert!(row(&buf, 7).starts_with("  Contents"));
        assert!(row(&buf, 8).starts_with("   ideas.md"));
        assert_eq!(buf[(2, 8)].bg, Color::Blue);
        assert!(row(&buf, 24).contains("✓ verified"));
        assert_eq!(buf[(2, 24)].fg, Color::LightGreen);
    }

    #[test]
    fn sizes_read_like_the_mockup() {
        assert_eq!(human_size(164), "164 B");
        assert_eq!(human_size(412 * 1024), "412 KiB");
        assert_eq!(human_size(8_800_000), "8.4 MiB");
        assert_eq!(human_size(96 * 1024 * 1024), "96 MiB");
    }

    #[test]
    fn text_is_cut_with_an_ellipsis() {
        assert_eq!(fit("abcdef", 4), "abc…");
        assert_eq!(fit_middle_trim("short.age", 20), "short.age");
        assert_eq!(
            fit_middle_trim("sample-backup-2026.tar.zst.age.sha256", 20),
            "sample-backu….sha256"
        );
        assert_eq!(fit_middle("abcdefghijkl", 9), "abcde…jkl");
        assert_eq!(fit_start_trim("/a/long/path/to/files", 12), "…th/to/files");
        assert_eq!(fit_start_trim("/short", 12), "/short");
        assert_eq!(fit("ab", 4), "ab  ");
        assert_eq!(fit_trim("abcdef", 6), "abcdef");
        assert_eq!(pad_left("7", 3), "  7");
        assert_eq!(wrap("one two three", 7), ["one two", "three"]);
        assert_eq!(tail("abcdef", 3), "def");
    }
}
