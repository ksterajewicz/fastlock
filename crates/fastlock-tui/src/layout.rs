//! Where things go on the screen.
//!
//! Both drawing and mouse handling use these numbers, so a click always lands
//! on the row that was drawn there.
//!
//! The grid follows the approved mockup (92 x 26):
//!
//! ```text
//! row 0           blank
//! row 1           fastlock  <screen>                              <status>
//! row 2           ──────────────────────────────────────────────────────────
//! row 3           blank
//! rows 4 .. h-3   list (from column 2) │ preview pane
//! row h-2         message
//! row h-1         key hints
//! ```

/// Below this width the preview pane is dropped and only the list is shown.
pub const PANE_MIN_WIDTH: u16 = 70;
/// Smallest size the interface draws at all.
pub const MIN_WIDTH: u16 = 30;
pub const MIN_HEIGHT: u16 = 12;
/// The left margin of every line.
pub const MARGIN: u16 = 2;
/// Width of the label column in the preview pane.
pub const LABEL_WIDTH: u16 = 14;

/// The positions of the parts of the screen for one terminal size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Geometry {
    pub width: u16,
    pub height: u16,
    pub header_y: u16,
    pub rule_y: u16,
    pub body_y: u16,
    pub body_h: u16,
    pub message_y: u16,
    pub footer_y: u16,
    /// First column of a list row (the selection bar starts here).
    pub list_x: u16,
    /// Width of the selection bar.
    pub list_w: u16,
    /// Column of the `│` between list and pane, when there is a pane.
    pub divider_x: Option<u16>,
    pub pane_x: u16,
    pub pane_w: u16,
}

impl Geometry {
    pub fn new(width: u16, height: u16) -> Self {
        let body_y = 4;
        let body_h = height.saturating_sub(6);
        let (list_w, divider_x, pane_x, pane_w) = if width >= PANE_MIN_WIDTH {
            // 46 columns for the list at 92 columns, 40 at 80.
            let left = (width / 2).clamp(30, 46);
            let pane_x = left + 5;
            (
                left - 3,
                Some(left + 2),
                pane_x,
                width.saturating_sub(pane_x + 2),
            )
        } else {
            (width.saturating_sub(2 * MARGIN), None, width, 0)
        };
        Geometry {
            width,
            height,
            header_y: 1,
            rule_y: 2,
            body_y,
            body_h,
            message_y: height.saturating_sub(2),
            footer_y: height.saturating_sub(1),
            list_x: MARGIN,
            list_w,
            divider_x,
            pane_x,
            pane_w,
        }
    }

    /// Whether the terminal is too small to draw the interface.
    pub fn too_small(&self) -> bool {
        self.width < MIN_WIDTH || self.height < MIN_HEIGHT
    }

    /// Whether the preview pane is shown.
    pub fn has_pane(&self) -> bool {
        self.divider_x.is_some() && self.pane_w > 0
    }

    /// The first column right of the list area; clicks there do not select.
    pub fn list_end(&self) -> u16 {
        self.divider_x.unwrap_or(self.width)
    }

    /// File list: the row of the first entry (the folder path is above it).
    pub fn files_top(&self) -> u16 {
        self.body_y + 1
    }

    /// File list: how many entries fit.
    pub fn files_rows(&self) -> usize {
        usize::from(self.body_h.saturating_sub(1))
    }

    /// Result screen: how many check rows are shown.
    pub fn checks_shown(&self, checks: usize) -> usize {
        checks.min(usize::from(self.body_h.saturating_sub(3)))
    }

    /// Result screen: the row of the first content entry.
    pub fn entries_top(&self, checks: usize) -> u16 {
        let shown = u16::try_from(self.checks_shown(checks)).unwrap_or(u16::MAX);
        self.body_y.saturating_add(shown).saturating_add(2)
    }

    /// Result screen: how many content entries fit.
    pub fn entries_rows(&self, checks: usize) -> usize {
        let end = self.body_y + self.body_h;
        usize::from(end.saturating_sub(self.entries_top(checks)))
    }
}

/// The scroll offset that keeps `sel` visible in a list of `len` rows of
/// which `rows` fit, starting from the previous `offset`.
pub fn keep_visible(sel: usize, offset: usize, rows: usize, len: usize) -> usize {
    if rows == 0 {
        return sel;
    }
    let offset = if sel < offset {
        sel
    } else if sel >= offset + rows {
        sel + 1 - rows
    } else {
        offset
    };
    offset.min(len.saturating_sub(rows))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_mockup_grid_at_92_by_26() {
        let g = Geometry::new(92, 26);
        assert_eq!(g.list_w, 43);
        assert_eq!(g.divider_x, Some(48));
        assert_eq!(g.pane_x, 51);
        assert_eq!(g.pane_w, 39);
        assert_eq!(g.body_h, 20);
        assert_eq!(g.message_y, 24);
        assert_eq!(g.footer_y, 25);
    }

    #[test]
    fn eighty_columns_keep_the_pane() {
        let g = Geometry::new(80, 24);
        assert!(g.has_pane());
        assert_eq!(g.list_w, 37);
        assert_eq!(g.pane_w, 33);
    }

    #[test]
    fn a_narrow_terminal_shows_only_the_list() {
        let g = Geometry::new(60, 24);
        assert!(!g.has_pane());
        assert_eq!(g.list_w, 56);
    }

    #[test]
    fn scrolling_keeps_the_selection_in_view() {
        assert_eq!(keep_visible(0, 0, 10, 30), 0);
        assert_eq!(keep_visible(12, 0, 10, 30), 3);
        assert_eq!(keep_visible(5, 3, 10, 30), 3);
        assert_eq!(keep_visible(2, 3, 10, 30), 2);
        assert_eq!(keep_visible(4, 20, 10, 5), 0);
    }
}
