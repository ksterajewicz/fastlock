//! Listing a folder, and turning typed paths into real ones.

use std::fs;
use std::io;
use std::path::{Path, PathBuf, MAIN_SEPARATOR};

/// One entry of a folder listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    /// The name as shown. Names that are not valid UTF-8 are shown lossily.
    pub name: String,
    /// The real path.
    pub path: PathBuf,
    /// Whether this is a folder (a link to a folder counts as one).
    pub is_dir: bool,
    /// The size in bytes; 0 for folders.
    pub size: u64,
}

impl Item {
    /// The name as shown in the list: folders end with `/`.
    pub fn label(&self) -> String {
        if self.is_dir {
            format!("{}/", self.name)
        } else {
            self.name.clone()
        }
    }
}

/// Lists a folder: folders first, then files, each sorted by name. Hidden
/// entries (names starting with `.`) are left out.
pub fn read_dir(dir: &Path) -> io::Result<Vec<Item>> {
    let mut items = Vec::new();
    for entry in fs::read_dir(dir)? {
        let Ok(entry) = entry else { continue };
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let path = entry.path();
        // Follow links, so a link to a folder can be entered. A broken link
        // falls back to the link itself.
        let metadata = fs::metadata(&path).or_else(|_| entry.metadata());
        let (is_dir, size) = match metadata {
            Ok(m) if m.is_dir() => (true, 0),
            Ok(m) => (false, m.len()),
            Err(_) => (false, 0),
        };
        items.push(Item {
            name,
            path,
            is_dir,
            size,
        });
    }
    sort_items(&mut items);
    Ok(items)
}

/// Sorts folders first, then by name ignoring case; equal names in different
/// case keep a stable order.
pub fn sort_items(items: &mut [Item]) {
    items.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            .then_with(|| a.name.cmp(&b.name))
    });
}

/// Expands `~` at the start of a typed path to the home folder.
pub fn expand_tilde(text: &str, home: Option<&Path>) -> PathBuf {
    if let Some(home) = home {
        if text == "~" {
            return home.to_path_buf();
        }
        if let Some(rest) = text
            .strip_prefix("~/")
            .or_else(|| text.strip_prefix(&format!("~{MAIN_SEPARATOR}")))
        {
            return home.join(rest);
        }
    }
    PathBuf::from(text)
}

/// Turns a typed path into a real one: `~` is expanded, and a relative path
/// is taken relative to `base` (the folder shown in the file list).
pub fn resolve(text: &str, base: &Path, home: Option<&Path>) -> PathBuf {
    let path = expand_tilde(text.trim(), home);
    if path.is_relative() {
        base.join(path)
    } else {
        path
    }
}

/// Shows a path with the home folder written as `~`.
pub fn display_path(path: &Path, home: Option<&Path>) -> String {
    if let Some(home) = home {
        if let Ok(rest) = path.strip_prefix(home) {
            if rest.as_os_str().is_empty() {
                return "~".to_string();
            }
            return format!("~{MAIN_SEPARATOR}{}", rest.display());
        }
    }
    path.display().to_string()
}

/// The last part of a path, for showing.
pub fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(name: &str, is_dir: bool) -> Item {
        Item {
            name: name.to_string(),
            path: PathBuf::from("/sample").join(name),
            is_dir,
            size: 0,
        }
    }

    #[test]
    fn folders_come_first_then_names_in_order() {
        let mut items = vec![
            item("zeta.age", false),
            item("photos", true),
            item("Alpha.age", false),
            item("archive", true),
            item("beta.age", false),
        ];
        sort_items(&mut items);
        let names: Vec<_> = items.iter().map(Item::label).collect();
        assert_eq!(
            names,
            ["archive/", "photos/", "Alpha.age", "beta.age", "zeta.age"]
        );
    }

    #[test]
    fn a_real_folder_is_listed_without_hidden_entries() {
        let dir = std::env::temp_dir().join(format!("fastlock-tui-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("inner")).unwrap();
        fs::write(dir.join("notes.txt.age"), b"sample").unwrap();
        fs::write(dir.join(".hidden"), b"sample").unwrap();

        let items = read_dir(&dir).unwrap();
        let _ = fs::remove_dir_all(&dir);

        let names: Vec<_> = items.iter().map(Item::label).collect();
        assert_eq!(names, ["inner/", "notes.txt.age"]);
        assert_eq!(items[1].size, 6);
    }

    #[test]
    fn a_tilde_expands_to_the_home_folder() {
        let home = Path::new("/home/sample");
        assert_eq!(expand_tilde("~", Some(home)), PathBuf::from("/home/sample"));
        assert_eq!(
            expand_tilde("~/keys/sample.txt", Some(home)),
            PathBuf::from("/home/sample/keys/sample.txt")
        );
        assert_eq!(expand_tilde("~other", Some(home)), PathBuf::from("~other"));
        assert_eq!(expand_tilde("~/x", None), PathBuf::from("~/x"));
    }

    #[test]
    fn a_relative_path_is_taken_from_the_shown_folder() {
        let base = Path::new("/sample/files");
        assert_eq!(
            resolve("key.txt", base, None),
            PathBuf::from("/sample/files/key.txt")
        );
        assert_eq!(
            resolve(" /abs/key.txt ", base, None),
            PathBuf::from("/abs/key.txt")
        );
    }

    #[test]
    fn the_home_folder_is_shown_as_a_tilde() {
        let home = Path::new("/home/sample");
        assert_eq!(display_path(Path::new("/home/sample"), Some(home)), "~");
        assert_eq!(
            display_path(Path::new("/home/sample/vault"), Some(home)),
            format!("~{MAIN_SEPARATOR}vault")
        );
        assert_eq!(
            display_path(Path::new("/srv/data"), Some(home)),
            "/srv/data"
        );
    }
}
