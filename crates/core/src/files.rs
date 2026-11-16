//! The file shelf: files and folders dropped on the notch wait here until
//! they are dragged out into another window.
//!
//! Only the paths are remembered. The files themselves stay where they are
//! (a file can be gigabytes); one that has been moved or deleted since is
//! shown as missing.

use image::{ImageFormat, Rgba, RgbaImage};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

/// How many entries the shelf keeps; the oldest fall off.
pub const FILES_LIMIT: usize = 50;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileEntry {
    pub id: u64,
    /// Absolute path.
    pub path: String,
    /// File name for the list.
    pub name: String,
    /// Unix time in milliseconds.
    pub added_at: u64,
    #[serde(default)]
    pub is_dir: bool,
    /// Size in bytes (0 for folders).
    #[serde(default)]
    pub size: u64,
    /// The path no longer exists (moved, deleted, a drive unplugged).
    #[serde(default)]
    pub missing: bool,
}

#[derive(Debug, Clone, Default)]
pub struct FileShelf {
    entries: Vec<FileEntry>,
    next_id: u64,
}

/// Windows paths differ in case and slash direction and still mean one file.
fn key(path: &str) -> String {
    path.replace('/', "\\").trim_end_matches('\\').to_lowercase()
}

fn name_of(path: &str) -> String {
    let trimmed = path.trim_end_matches(['\\', '/']);
    let name = trimmed.rsplit(['\\', '/']).next().unwrap_or(trimmed);
    if name.is_empty() { path.to_string() } else { name.to_string() }
}

impl FileShelf {
    pub fn new() -> Self {
        Self { entries: Vec::new(), next_id: 1 }
    }

    /// Reads the list saved by [`FileShelf::save`]; a missing or broken file
    /// gives an empty shelf.
    pub fn load(path: &Path) -> Self {
        let entries: Vec<FileEntry> =
            fs::read(path).ok().and_then(|raw| serde_json::from_slice(&raw).ok()).unwrap_or_default();
        let mut shelf = Self { next_id: entries.iter().map(|e| e.id).max().unwrap_or(0) + 1, entries };
        shelf.entries.truncate(FILES_LIMIT);
        shelf.refresh();
        shelf
    }

    pub fn save(&self, path: &Path) {
        if self.entries.is_empty() {
            let _ = fs::remove_file(path);
        } else if let Ok(json) = serde_json::to_vec_pretty(&self.entries) {
            let _ = crate::settings::write_atomic(path, &json);
        }
    }

    pub fn entries(&self) -> &[FileEntry] {
        &self.entries
    }

    pub fn get(&self, id: u64) -> Option<&FileEntry> {
        self.entries.iter().find(|e| e.id == id)
    }

    /// Puts paths on top of the shelf, in the order given. A path already
    /// there moves to the top instead of appearing twice. Returns how many
    /// were taken (relative paths are not: they mean nothing later).
    pub fn add<S: AsRef<str>>(&mut self, paths: &[S], now_ms: u64) -> usize {
        let mut taken = 0;
        for path in paths.iter().rev() {
            let path = path.as_ref().trim();
            if path.is_empty() || !Path::new(path).is_absolute() {
                continue;
            }
            let k = key(path);
            let id = match self.entries.iter().position(|e| key(&e.path) == k) {
                Some(pos) => self.entries.remove(pos).id,
                None => {
                    self.next_id += 1;
                    self.next_id - 1
                }
            };
            let mut entry = FileEntry {
                id,
                path: path.to_string(),
                name: name_of(path),
                added_at: now_ms,
                is_dir: false,
                size: 0,
                missing: false,
            };
            stat(&mut entry);
            self.entries.insert(0, entry);
            taken += 1;
        }
        self.entries.truncate(FILES_LIMIT);
        taken
    }

    pub fn remove(&mut self, id: u64) -> bool {
        let before = self.entries.len();
        self.entries.retain(|e| e.id != id);
        before != self.entries.len()
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Looks at the disk again: sizes, and which files are gone. Returns
    /// whether anything changed.
    pub fn refresh(&mut self) -> bool {
        let mut changed = false;
        for entry in &mut self.entries {
            let before = (entry.is_dir, entry.size, entry.missing);
            stat(entry);
            changed |= before != (entry.is_dir, entry.size, entry.missing);
        }
        changed
    }
}

fn stat(entry: &mut FileEntry) {
    match fs::metadata(&entry.path) {
        Ok(meta) => {
            entry.missing = false;
            entry.is_dir = meta.is_dir();
            entry.size = if meta.is_dir() { 0 } else { meta.len() };
        }
        Err(_) => entry.missing = true,
    }
}

/// The picture that follows the cursor while files are dragged out of the
/// notch: a plain sheet of paper with a folded corner. Written once.
pub fn write_drag_icon(path: &Path) -> Result<(), String> {
    if path.exists() {
        return Ok(());
    }
    const W: u32 = 48;
    const H: u32 = 60;
    const FOLD: u32 = 16;
    let paper = Rgba([238, 240, 244, 255]);
    let edge = Rgba([120, 126, 138, 255]);
    let fold = Rgba([196, 201, 210, 255]);
    let mut img = RgbaImage::from_pixel(W, H, Rgba([0, 0, 0, 0]));
    for y in 0..H {
        for x in 0..W {
            // The top right corner is cut off along a diagonal.
            let cut = x + FOLD >= W && y < FOLD && (x + FOLD - W) > y;
            if cut {
                continue;
            }
            let on_diagonal = x + FOLD >= W && y < FOLD && (x + FOLD - W) == y;
            let under_fold = x + FOLD >= W && y < FOLD;
            let border = x == 0 || y == 0 || x == W - 1 || y == H - 1 || on_diagonal;
            img.put_pixel(x, y, if border { edge } else if under_fold { fold } else { paper });
        }
    }
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    img.save_with_format(path, ImageFormat::Png).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("notch-files-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn text(path: &Path) -> String {
        path.to_string_lossy().into_owned()
    }

    #[test]
    fn adds_on_top_and_moves_repeats() {
        let dir = temp("add");
        let (a, b) = (dir.join("a.txt"), dir.join("b.bin"));
        fs::write(&a, b"12345").unwrap();
        fs::write(&b, b"1").unwrap();
        let mut shelf = FileShelf::new();
        // Several files dropped at once keep their order.
        assert_eq!(shelf.add(&[text(&a), text(&b), text(&dir)], 10), 3);
        let names: Vec<_> = shelf.entries().iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names[..2], ["a.txt", "b.bin"]);
        let first = &shelf.entries()[0];
        assert_eq!((first.size, first.is_dir, first.missing), (5, false, false));
        assert!(shelf.entries()[2].is_dir);
        // Dropped again: one entry, on top, same id.
        let id_b = shelf.entries()[1].id;
        assert_eq!(shelf.add(&[text(&b)], 20), 1);
        assert_eq!(shelf.entries().len(), 3);
        assert_eq!((shelf.entries()[0].id, shelf.entries()[0].added_at), (id_b, 20));
        // Relative and empty paths are not taken.
        assert_eq!(shelf.add(&["notes.txt", "  "], 30), 0);
        assert!(shelf.remove(id_b));
        assert!(!shelf.remove(id_b));
        shelf.clear();
        assert!(shelf.entries().is_empty());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn same_file_in_another_spelling_is_one_entry() {
        assert_eq!(key(r"C:\Users\Me\File.TXT"), key("c:/users/me/file.txt"));
        assert_eq!(key(r"D:\Folder\"), key(r"d:\folder"));
        assert_eq!(name_of(r"C:\Docs\Отчёт за май.xlsx"), "Отчёт за май.xlsx");
        assert_eq!(name_of(r"D:\Folder\"), "Folder");
        assert_eq!(name_of("/home/me/a.txt"), "a.txt");
    }

    #[test]
    fn survives_restart_and_notices_missing_files() {
        let dir = temp("persist");
        let file = dir.join("report.pdf");
        fs::write(&file, b"pdf").unwrap();
        let index = dir.join("files.json");
        let mut shelf = FileShelf::new();
        shelf.add(&[text(&file)], 5);
        shelf.save(&index);

        let mut loaded = FileShelf::load(&index);
        assert_eq!(loaded.entries(), shelf.entries());
        assert!(!loaded.refresh(), "nothing changed on disk");
        fs::remove_file(&file).unwrap();
        assert!(loaded.refresh());
        assert!(loaded.entries()[0].missing);
        // New entries continue the ids.
        fs::write(&file, b"again").unwrap();
        let other = dir.join("other.txt");
        fs::write(&other, b"x").unwrap();
        loaded.add(&[text(&other)], 6);
        assert_ne!(loaded.entries()[0].id, loaded.entries()[1].id);

        // An emptied shelf leaves no file behind; a broken one reads as empty.
        loaded.clear();
        loaded.save(&index);
        assert!(!index.exists());
        fs::write(&index, b"{ nope").unwrap();
        assert!(FileShelf::load(&index).entries().is_empty());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_ring_keeps_the_newest() {
        let dir = temp("ring");
        let paths: Vec<String> = (0..FILES_LIMIT + 5).map(|i| text(&dir.join(format!("f{i}")))).collect();
        let mut shelf = FileShelf::new();
        for (i, p) in paths.iter().enumerate() {
            shelf.add(std::slice::from_ref(p), i as u64);
        }
        assert_eq!(shelf.entries().len(), FILES_LIMIT);
        assert_eq!(shelf.entries()[0].name, format!("f{}", FILES_LIMIT + 4));
        assert!(shelf.entries()[0].missing, "never existed");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn drag_icon_is_written_once() {
        let dir = temp("icon");
        let icon = dir.join("cache").join("file.png");
        write_drag_icon(&icon).unwrap();
        assert_eq!(image::image_dimensions(&icon).unwrap(), (48, 60));
        let stamp = fs::metadata(&icon).unwrap().modified().unwrap();
        write_drag_icon(&icon).unwrap();
        assert_eq!(fs::metadata(&icon).unwrap().modified().unwrap(), stamp);
        fs::remove_dir_all(&dir).unwrap();
    }
}
