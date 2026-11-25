//! The colors of the terminal interface.
//!
//! Every color the interface uses comes from [`Theme`]. The default theme
//! uses the terminal's own palette (the sixteen ANSI colors), so the program
//! follows the colors the person already chose for their terminal. A theme
//! file will replace these values later; its format is not decided yet.

use ratatui::style::{Color, Modifier, Style};

/// The named styles of the interface.
#[derive(Clone, Copy)]
pub struct Theme {
    /// Ordinary text.
    pub text: Style,
    /// Group headings, titles and the program name.
    pub heading: Style,
    /// Secondary text: states, labels, hints, rules and borders.
    pub secondary: Style,
    /// Shortcut letters, key names and progress.
    pub accent: Style,
    /// The selected row.
    pub selection: Style,
    /// Something that worked.
    pub ok: Style,
    /// Something to look at.
    pub warning: Style,
    /// Something that failed.
    pub error: Style,
}

impl Theme {
    /// The theme built from the terminal's own palette.
    pub const fn terminal() -> Self {
        Theme {
            text: Style::new(),
            heading: Style::new().add_modifier(Modifier::BOLD),
            // Secondary text is the terminal's own text color, dimmed. A fixed
            // gray would vanish on palettes where gray is close to the text
            // color or to the background.
            secondary: Style::new().add_modifier(Modifier::DIM),
            accent: Style::new().fg(Color::LightBlue),
            selection: Style::new()
                .fg(Color::White)
                .bg(Color::Blue)
                .add_modifier(Modifier::BOLD),
            // The bright variants: on many palettes the plain ones are too
            // dark to read on a dark background.
            ok: Style::new().fg(Color::LightGreen),
            warning: Style::new().fg(Color::LightYellow),
            error: Style::new().fg(Color::LightRed),
        }
    }
}

impl Default for Theme {
    fn default() -> Self {
        Theme::terminal()
    }
}
