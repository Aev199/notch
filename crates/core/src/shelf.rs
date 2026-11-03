//! Screenshot shelf: clipboard images saved as PNG files in a cache folder,
//! kept as a small ring (oldest files are deleted automatically).

use base64::Engine;
use image::{imageops::FilterType, ImageBuffer, ImageFormat, Rgba};
use serde::Serialize;
use std::fs;
use std::hash::{Hash, Hasher};
use std::io::Cursor;
use std::path::{Path, PathBuf};

/// Longest side of a shelf thumbnail, in pixels.
pub const THUMB_SIZE: u32 = 192;
const PREFIX: &str = "shot-";
const THUMB_SUFFIX: &str = ".thumb.png";

#[derive(Debug, Clone, Serialize)]
pub struct ShelfItem {
    /// File stem, e.g. `shot-1730612345123`; unique and sortable by time.
    pub id: String,
    /// Absolute path of the full-size PNG.
    pub path: String,
    /// Absolute path of the thumbnail PNG (used as drag icon).
    pub thumb_path: String,
    /// `data:image/png;base64,...` of the thumbnail for the UI.
    pub thumb: String,
    pub width: u32,
    pub height: u32,
    pub created_at: u64,
}

#[derive(Debug)]
pub struct Shelf {
    dir: PathBuf,
    limit: usize,
    items: Vec<ShelfItem>,
    last_hash: Option<u64>,
}

/// Cheap fingerprint of an RGBA buffer, used to ignore the same image being
/// put on the clipboard twice (for example, when we copy it back ourselves).
pub fn fingerprint(width: u32, height: u32, rgba: &[u8]) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    (width, height, rgba.len()).hash(&mut hasher);
    // Sample at most ~256k bytes: plenty to tell screenshots apart, still fast on 4K.
    let step = (rgba.len() / 262_144).max(1);
    for chunk in rgba.chunks(4).step_by(step) {
        chunk.hash(&mut hasher);
    }
    hasher.finish()
}

fn data_url(png: &[u8]) -> String {
    format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(png))
}

impl Shelf {
    /// Opens (and creates) the shelf folder and loads the screenshots that are
    /// already there, newest first.
    pub fn open(dir: impl Into<PathBuf>, limit: usize) -> std::io::Result<Self> {
        let dir = dir.into();
        fs::create_dir_all(&dir)?;
        let mut shelf = Self { dir, limit: limit.max(1), items: Vec::new(), last_hash: None };
        shelf.reload();
        shelf.enforce_limit();
        Ok(shelf)
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn items(&self) -> &[ShelfItem] {
        &self.items
    }

    pub fn get(&self, id: &str) -> Option<&ShelfItem> {
        self.items.iter().find(|i| i.id == id)
    }

    pub fn set_limit(&mut self, limit: usize) {
        self.limit = limit.max(1);
        self.enforce_limit();
    }

    /// Marks an image as "already seen" so the next clipboard update with the
    /// same pixels is ignored.
    pub fn remember(&mut self, hash: u64) {
        self.last_hash = Some(hash);
    }

    fn reload(&mut self) {
        let mut items = Vec::new();
        if let Ok(read) = fs::read_dir(&self.dir) {
            for entry in read.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if !name.starts_with(PREFIX) || !name.ends_with(".png") || name.ends_with(THUMB_SUFFIX) {
                    continue;
                }
                let stem = name.trim_end_matches(".png").to_string();
                if let Some(item) = self.load_item(&stem) {
                    items.push(item);
                }
            }
        }
        items.sort_by(|a, b| b.id.cmp(&a.id));
        self.items = items;
    }

    fn paths(&self, stem: &str) -> (PathBuf, PathBuf) {
        (self.dir.join(format!("{stem}.png")), self.dir.join(format!("{stem}{THUMB_SUFFIX}")))
    }

    fn load_item(&self, stem: &str) -> Option<ShelfItem> {
        let (full, thumb_path) = self.paths(stem);
        let (width, height) = image::image_dimensions(&full).ok()?;
        if !thumb_path.exists() {
            let img = image::open(&full).ok()?;
            img.thumbnail(THUMB_SIZE, THUMB_SIZE).save_with_format(&thumb_path, ImageFormat::Png).ok()?;
        }
        let thumb_png = fs::read(&thumb_path).ok()?;
        let created_at = stem.trim_start_matches(PREFIX).split('-').next()?.parse().unwrap_or(0);
        Some(ShelfItem {
            id: stem.to_string(),
            path: full.to_string_lossy().to_string(),
            thumb_path: thumb_path.to_string_lossy().to_string(),
            thumb: data_url(&thumb_png),
            width,
            height,
            created_at,
        })
    }

    /// Saves a clipboard image (raw RGBA) to the shelf. Returns `Ok(None)`
    /// when the same image was the last one seen.
    pub fn add_rgba(&mut self, width: u32, height: u32, rgba: Vec<u8>, now_ms: u64) -> Result<Option<ShelfItem>, String> {
        let hash = fingerprint(width, height, &rgba);
        if self.last_hash == Some(hash) {
            return Ok(None);
        }
        let buffer: ImageBuffer<Rgba<u8>, Vec<u8>> =
            ImageBuffer::from_raw(width, height, rgba).ok_or("размер изображения не совпадает с данными")?;
        let mut stem = format!("{PREFIX}{now_ms:013}");
        let mut n = 1;
        while self.paths(&stem).0.exists() {
            stem = format!("{PREFIX}{now_ms:013}-{n}");
            n += 1;
        }
        let (full, thumb_path) = self.paths(&stem);
        buffer.save_with_format(&full, ImageFormat::Png).map_err(|e| e.to_string())?;
        let img = image::DynamicImage::ImageRgba8(buffer);
        let thumb = if width <= THUMB_SIZE && height <= THUMB_SIZE {
            img
        } else {
            img.resize(THUMB_SIZE, THUMB_SIZE, FilterType::Triangle)
        };
        let mut thumb_png = Vec::new();
        thumb.write_to(&mut Cursor::new(&mut thumb_png), ImageFormat::Png).map_err(|e| e.to_string())?;
        fs::write(&thumb_path, &thumb_png).map_err(|e| e.to_string())?;
        self.last_hash = Some(hash);
        let item = ShelfItem {
            id: stem,
            path: full.to_string_lossy().to_string(),
            thumb_path: thumb_path.to_string_lossy().to_string(),
            thumb: data_url(&thumb_png),
            width,
            height,
            created_at: now_ms,
        };
        self.items.insert(0, item.clone());
        self.enforce_limit();
        Ok(Some(item))
    }

    /// Reads a stored screenshot back as RGBA (for copying it to the clipboard).
    pub fn load_rgba(&self, id: &str) -> Result<(u32, u32, Vec<u8>), String> {
        let item = self.get(id).ok_or("скриншот не найден")?;
        let img = image::open(&item.path).map_err(|e| e.to_string())?.into_rgba8();
        Ok((img.width(), img.height(), img.into_raw()))
    }

    pub fn remove(&mut self, id: &str) -> bool {
        let Some(pos) = self.items.iter().position(|i| i.id == id) else { return false };
        let item = self.items.remove(pos);
        let _ = fs::remove_file(&item.path);
        let _ = fs::remove_file(&item.thumb_path);
        true
    }

    pub fn clear(&mut self) {
        for item in std::mem::take(&mut self.items) {
            let _ = fs::remove_file(&item.path);
            let _ = fs::remove_file(&item.thumb_path);
        }
    }

    fn enforce_limit(&mut self) {
        while self.items.len() > self.limit {
            if let Some(item) = self.items.pop() {
                let _ = fs::remove_file(&item.path);
                let _ = fs::remove_file(&item.thumb_path);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("notch-shelf-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    fn pixels(w: u32, h: u32, seed: u8) -> Vec<u8> {
        (0..w * h * 4).map(|i| (i as u8).wrapping_add(seed)).collect()
    }

    #[test]
    fn saves_png_thumbnail_and_dedupes() {
        let dir = temp_dir("save");
        let mut shelf = Shelf::open(&dir, 3).unwrap();
        let item = shelf.add_rgba(400, 200, pixels(400, 200, 1), 1000).unwrap().unwrap();
        assert!(Path::new(&item.path).exists());
        assert!(Path::new(&item.thumb_path).exists());
        assert_eq!(image::image_dimensions(&item.thumb_path).unwrap(), (192, 96));
        assert!(item.thumb.starts_with("data:image/png;base64,"));
        assert!(shelf.add_rgba(400, 200, pixels(400, 200, 1), 2000).unwrap().is_none());
        let (w, h, rgba) = shelf.load_rgba(&item.id).unwrap();
        assert_eq!((w, h), (400, 200));
        assert_eq!(rgba, pixels(400, 200, 1));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn ring_deletes_oldest_files_and_reloads() {
        let dir = temp_dir("ring");
        let mut shelf = Shelf::open(&dir, 2).unwrap();
        let first = shelf.add_rgba(8, 8, pixels(8, 8, 1), 1).unwrap().unwrap();
        shelf.add_rgba(8, 8, pixels(8, 8, 2), 2).unwrap();
        shelf.add_rgba(8, 8, pixels(8, 8, 3), 3).unwrap();
        assert_eq!(shelf.items().len(), 2);
        assert!(!Path::new(&first.path).exists());
        assert!(!Path::new(&first.thumb_path).exists());

        let reopened = Shelf::open(&dir, 2).unwrap();
        let ids: Vec<_> = reopened.items().iter().map(|i| i.created_at).collect();
        assert_eq!(ids, [3, 2]);

        let mut smaller = Shelf::open(&dir, 1).unwrap();
        assert_eq!(smaller.items().len(), 1);
        let id = smaller.items()[0].id.clone();
        assert!(smaller.remove(&id));
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 0);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn same_millisecond_gets_unique_name() {
        let dir = temp_dir("same-ms");
        let mut shelf = Shelf::open(&dir, 5).unwrap();
        let a = shelf.add_rgba(4, 4, pixels(4, 4, 1), 7).unwrap().unwrap();
        let b = shelf.add_rgba(4, 4, pixels(4, 4, 2), 7).unwrap().unwrap();
        assert_ne!(a.id, b.id);
        shelf.clear();
        assert!(shelf.items().is_empty());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn rejects_wrong_buffer_size() {
        let dir = temp_dir("bad");
        let mut shelf = Shelf::open(&dir, 5).unwrap();
        assert!(shelf.add_rgba(10, 10, vec![0; 12], 1).is_err());
        fs::remove_dir_all(&dir).unwrap();
    }
}
