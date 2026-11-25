//! A one-line text field.
//!
//! A field made with [`TextField::secret`] holds a passphrase. Its buffer is
//! allocated once with a fixed capacity, so typing never moves the text to a
//! new allocation and leaves no copy behind. The buffer is zeroed when the
//! passphrase is taken out, when the field is cleared, and when it is dropped.

use std::fmt;

use fastlock_core::SecretString;
use zeroize::Zeroize;

/// The most bytes a secret field accepts. Larger input is ignored, so the
/// buffer never has to grow.
pub const SECRET_CAPACITY: usize = 1024;

/// A one-line text field with a cursor.
#[derive(Default)]
pub struct TextField {
    value: String,
    /// The cursor position, counted in characters.
    cursor: usize,
    secret: bool,
}

impl TextField {
    /// A field that starts with `initial`, with the cursor at the end.
    pub fn new(initial: &str) -> Self {
        let mut field = TextField::default();
        field.insert_str(initial);
        field
    }

    /// An empty field for a passphrase.
    pub fn secret() -> Self {
        TextField {
            value: String::with_capacity(SECRET_CAPACITY),
            cursor: 0,
            secret: true,
        }
    }

    /// Whether this field holds a passphrase.
    pub fn is_secret(&self) -> bool {
        self.secret
    }

    /// The text. For a secret field, callers show only its length.
    pub fn text(&self) -> &str {
        &self.value
    }

    /// The cursor position in characters.
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// The number of characters.
    pub fn char_count(&self) -> usize {
        self.value.chars().count()
    }

    /// Whether the field is empty.
    pub fn is_empty(&self) -> bool {
        self.value.is_empty()
    }

    /// Inserts one character at the cursor. Control characters are ignored.
    pub fn insert(&mut self, c: char) {
        if c.is_control() {
            return;
        }
        if self.secret && self.value.len() + c.len_utf8() > SECRET_CAPACITY {
            return;
        }
        let at = self.byte_index(self.cursor);
        self.value.insert(at, c);
        self.cursor += 1;
    }

    /// Inserts text at the cursor, for example from a paste. Line breaks and
    /// other control characters are dropped.
    pub fn insert_str(&mut self, text: &str) {
        for c in text.chars() {
            self.insert(c);
        }
    }

    /// Deletes the character before the cursor. Returns `false` when there
    /// was nothing to delete.
    pub fn backspace(&mut self) -> bool {
        if self.cursor == 0 {
            return false;
        }
        let at = self.byte_index(self.cursor - 1);
        self.value.remove(at);
        self.cursor -= 1;
        true
    }

    /// Deletes the character under the cursor.
    pub fn delete(&mut self) {
        if self.cursor < self.char_count() {
            let at = self.byte_index(self.cursor);
            self.value.remove(at);
        }
    }

    /// Moves the cursor one character to the left.
    pub fn left(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    /// Moves the cursor one character to the right.
    pub fn right(&mut self) {
        self.cursor = (self.cursor + 1).min(self.char_count());
    }

    /// Moves the cursor to the start.
    pub fn home(&mut self) {
        self.cursor = 0;
    }

    /// Moves the cursor to the end.
    pub fn end(&mut self) {
        self.cursor = self.char_count();
    }

    /// Empties the field and zeroes its buffer.
    pub fn clear(&mut self) {
        self.value.zeroize();
        self.cursor = 0;
    }

    /// Takes the text out as a secret and empties the field.
    ///
    /// The secret gets its own allocation of exactly the right size; the
    /// field's buffer is then zeroed.
    pub fn take_secret(&mut self) -> SecretString {
        let secret = SecretString::from(self.value.as_str());
        self.clear();
        secret
    }

    fn byte_index(&self, chars: usize) -> usize {
        self.value
            .char_indices()
            .nth(chars)
            .map_or(self.value.len(), |(i, _)| i)
    }
}

impl Drop for TextField {
    fn drop(&mut self) {
        self.value.zeroize();
    }
}

impl fmt::Debug for TextField {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.secret {
            write!(f, "TextField(secret, {} chars)", self.char_count())
        } else {
            write!(f, "TextField({:?}, cursor {})", self.value, self.cursor)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fastlock_core::ExposeSecret;

    #[test]
    fn typing_inserts_at_the_cursor() {
        let mut field = TextField::new("ac");
        field.left();
        field.insert('b');
        assert_eq!(field.text(), "abc");
        assert_eq!(field.cursor(), 2);
        field.home();
        field.insert('>');
        field.end();
        field.insert('<');
        assert_eq!(field.text(), ">abc<");
    }

    #[test]
    fn backspace_and_delete_remove_characters() {
        let mut field = TextField::new("näme");
        assert!(field.backspace());
        assert_eq!(field.text(), "näm");
        field.home();
        assert!(!field.backspace());
        field.delete();
        assert_eq!(field.text(), "äm");
        field.end();
        field.delete();
        assert_eq!(field.text(), "äm");
    }

    #[test]
    fn the_cursor_stays_inside_the_text() {
        let mut field = TextField::new("ab");
        field.right();
        field.right();
        assert_eq!(field.cursor(), 2);
        field.left();
        field.left();
        field.left();
        assert_eq!(field.cursor(), 0);
    }

    #[test]
    fn control_characters_from_a_paste_are_dropped() {
        let mut field = TextField::new("");
        field.insert_str("~/keys/sample.txt\n");
        assert_eq!(field.text(), "~/keys/sample.txt");
    }

    #[test]
    fn taking_a_secret_empties_the_field() {
        let mut field = TextField::secret();
        field.insert_str("sample passphrase");
        let secret = field.take_secret();
        assert_eq!(secret.expose_secret(), "sample passphrase");
        assert!(field.is_empty());
        assert_eq!(field.cursor(), 0);
    }

    #[test]
    fn a_secret_field_never_grows_past_its_capacity() {
        let mut field = TextField::secret();
        for _ in 0..SECRET_CAPACITY + 10 {
            field.insert('x');
        }
        assert_eq!(field.char_count(), SECRET_CAPACITY);
    }

    #[test]
    fn debug_output_hides_a_secret() {
        let mut field = TextField::secret();
        field.insert_str("hidden");
        let shown = format!("{field:?}");
        assert!(!shown.contains("hidden"));
    }
}
