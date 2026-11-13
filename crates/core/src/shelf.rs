//! Screenshot shelf: clipboard images saved as PNG files in a cache folder,
//! kept as a small ring (oldest files are deleted automatically).

use base64::Engine;
use image::{imageops::FilterType, ImageBuffer, ImageFormat, Rgba};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::hash::{Hash, Hasher};
use std::io::Cursor;
use std::path::{Path, PathBuf};

/// Longest side of a shelf thumbnail, in pixels.
pub const THUMB_SIZE: u32 = 192;
const PREFIX: &str = "shot-";
const THUMB_SUFFIX: &str = ".thumb.png";
/// Which screenshot belongs to which group (screenshots keep living as
/// plain PNG files; this small index is all a group is).
const GROUPS_FILE: &str = "groups.json";
/// Safety cap for grouped screenshots, which the ring never deletes.
pub const MAX_GROUPED: usize = 300;

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
    /// Group id (`g-<ms>`) when the screenshot was put into a group.
    pub group: Option<String>,
}

#[derive(Debug)]
pub struct Shelf {
    dir: PathBuf,
    limit: usize,
    items: Vec<ShelfItem>,
    last_hash: Option<u64>,
    /// item id → group id
    groups: BTreeMap<String, String>,
    /// item id → pixel hash, filled lazily (only for same-size candidates).
    pixel_hashes: HashMap<String, [u8; 32]>,
}

/// Identity of an image's pixels (and size): equal for the same picture
/// whichever PNG encoder wrote it.
pub fn pixel_hash(width: u32, height: u32, rgba: &[u8]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(width.to_le_bytes());
    h.update(height.to_le_bytes());
    h.update(rgba);
    h.finalize().into()
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

/// A clipboard image turned into PNG bytes (full size and thumbnail).
#[derive(Debug)]
pub struct Encoded {
    pub hash: u64,
    pub pixels: [u8; 32],
    pub width: u32,
    pub height: u32,
    png: Vec<u8>,
    thumb: Vec<u8>,
}

/// PNG-encodes raw RGBA. Slow for big screenshots (hundreds of ms on 4K),
/// so it does not need — and must not hold — the shelf lock.
pub fn encode(width: u32, height: u32, rgba: Vec<u8>) -> Result<Encoded, String> {
    let hash = fingerprint(width, height, &rgba);
    let pixels = pixel_hash(width, height, &rgba);
    let buffer: ImageBuffer<Rgba<u8>, Vec<u8>> =
        ImageBuffer::from_raw(width, height, rgba).ok_or("размер изображения не совпадает с данными")?;
    let mut png = Vec::new();
    buffer.write_to(&mut Cursor::new(&mut png), ImageFormat::Png).map_err(|e| e.to_string())?;
    let img = image::DynamicImage::ImageRgba8(buffer);
    let thumb_img =
        if width <= THUMB_SIZE && height <= THUMB_SIZE { img } else { img.resize(THUMB_SIZE, THUMB_SIZE, FilterType::Triangle) };
    let mut thumb = Vec::new();
    thumb_img.write_to(&mut Cursor::new(&mut thumb), ImageFormat::Png).map_err(|e| e.to_string())?;
    Ok(Encoded { hash, pixels, width, height, png, thumb })
}

/// PNG bytes → `(width, height, rgba)`.
pub fn decode_png(png: &[u8]) -> Result<(u32, u32, Vec<u8>), String> {
    let img = image::load_from_memory_with_format(png, ImageFormat::Png).map_err(|e| format!("не PNG: {e}"))?.into_rgba8();
    Ok((img.width(), img.height(), img.into_raw()))
}

/// An edited screenshot arriving as PNG bytes (from the editor window): the
/// bytes are stored as they are, only the thumbnail and fingerprint are made.
pub fn encode_png(png: &[u8]) -> Result<Encoded, String> {
    let img = image::load_from_memory_with_format(png, ImageFormat::Png).map_err(|e| format!("не PNG: {e}"))?;
    let (width, height) = (img.width(), img.height());
    let rgba = img.to_rgba8();
    let hash = fingerprint(width, height, rgba.as_raw());
    let pixels = pixel_hash(width, height, rgba.as_raw());
    let thumb_img = if width <= THUMB_SIZE && height <= THUMB_SIZE {
        img
    } else {
        img.resize(THUMB_SIZE, THUMB_SIZE, FilterType::Triangle)
    };
    let mut thumb = Vec::new();
    thumb_img.write_to(&mut Cursor::new(&mut thumb), ImageFormat::Png).map_err(|e| e.to_string())?;
    Ok(Encoded { hash, pixels, width, height, png: png.to_vec(), thumb })
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
        let mut shelf = Self {
            dir,
            limit: limit.max(1),
            items: Vec::new(),
            last_hash: None,
            groups: BTreeMap::new(),
            pixel_hashes: HashMap::new(),
        };
        shelf.load_groups();
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
        for item in &mut items {
            item.group = self.groups.get(&item.id).cloned();
        }
        // Forget index entries whose files are gone.
        let before = self.groups.len();
        self.groups.retain(|id, _| items.iter().any(|i| &i.id == id));
        self.items = items;
        if before != self.groups.len() {
            self.save_groups();
        }
    }

    fn load_groups(&mut self) {
        self.groups = fs::read(self.dir.join(GROUPS_FILE))
            .ok()
            .and_then(|raw| serde_json::from_slice(&raw).ok())
            .unwrap_or_default();
    }

    fn save_groups(&self) {
        if let Ok(json) = serde_json::to_vec_pretty(&self.groups) {
            let _ = crate::settings::write_atomic(&self.dir.join(GROUPS_FILE), &json);
        }
    }

    fn sync_group_fields(&mut self) {
        for item in &mut self.items {
            item.group = self.groups.get(&item.id).cloned();
        }
        self.save_groups();
    }

    /// Groups whose size dropped to one stop being groups.
    fn dissolve_singletons(&mut self) {
        let mut counts: BTreeMap<String, usize> = BTreeMap::new();
        for g in self.groups.values() {
            *counts.entry(g.clone()).or_default() += 1;
        }
        self.groups.retain(|_, g| counts.get(g).copied().unwrap_or(0) > 1);
    }

    /// Drops `ids` onto `target` (a screenshot or a group id): the target's
    /// group grows, or a new group is made of the target and `ids`. Whole
    /// groups among `ids` are merged. Returns the resulting group id.
    pub fn group(&mut self, ids: &[String], target: &str, now_ms: u64) -> Result<String, String> {
        let is_group = |shelf: &Self, id: &str| shelf.groups.values().any(|g| g == id);
        let target_group = if is_group(self, target) {
            target.to_string()
        } else if let Some(g) = self.groups.get(target) {
            g.clone()
        } else if self.get(target).is_some() {
            let mut gid = format!("g-{now_ms}");
            let mut n = 1;
            while is_group(self, &gid) {
                gid = format!("g-{now_ms}-{n}");
                n += 1;
            }
            self.groups.insert(target.to_string(), gid.clone());
            gid
        } else {
            return Err("скриншот не найден".into());
        };
        for id in ids {
            if is_group(self, id) {
                let members: Vec<String> =
                    self.groups.iter().filter(|(_, g)| *g == id).map(|(item, _)| item.clone()).collect();
                for item in members {
                    self.groups.insert(item, target_group.clone());
                }
            } else if self.get(id).is_some() {
                self.groups.insert(id.clone(), target_group.clone());
            }
        }
        self.dissolve_singletons();
        self.sync_group_fields();
        Ok(target_group)
    }

    /// Takes one screenshot out of its group (it becomes a loose one again).
    pub fn ungroup(&mut self, id: &str) -> bool {
        if self.groups.remove(id).is_none() {
            return false;
        }
        self.dissolve_singletons();
        self.sync_group_fields();
        self.enforce_limit();
        true
    }

    /// Screenshots of a group, newest first.
    pub fn group_items(&self, gid: &str) -> Vec<&ShelfItem> {
        self.items.iter().filter(|i| i.group.as_deref() == Some(gid)).collect()
    }

    /// Deletes a whole group with its files.
    pub fn remove_group(&mut self, gid: &str) -> usize {
        let ids: Vec<String> = self.group_items(gid).iter().map(|i| i.id.clone()).collect();
        for id in &ids {
            self.remove(id);
        }
        ids.len()
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
            group: None,
        })
    }

    /// The same pixels were the last image seen (e.g. our own copy-back).
    pub fn is_duplicate(&self, hash: u64) -> bool {
        self.last_hash == Some(hash)
    }

    /// Saves a clipboard image (raw RGBA) to the shelf. Returns `Ok(None)`
    /// when the same image was the last one seen. Convenience wrapper; the
    /// app encodes with [`encode`] outside its lock and calls [`Shelf::insert`].
    pub fn add_rgba(&mut self, width: u32, height: u32, rgba: Vec<u8>, now_ms: u64) -> Result<Option<ShelfItem>, String> {
        let hash = fingerprint(width, height, &rgba);
        if self.is_duplicate(hash) {
            return Ok(None);
        }
        let encoded = encode(width, height, rgba)?;
        self.insert(encoded, now_ms).map(Some)
    }

    /// Writes an encoded screenshot to disk and puts it on top of the shelf.
    /// An item showing exactly these pixels, if the shelf already has one.
    fn find_same(&mut self, enc: &Encoded) -> Option<String> {
        let candidates: Vec<(String, String)> = self
            .items
            .iter()
            .filter(|i| i.width == enc.width && i.height == enc.height)
            .map(|i| (i.id.clone(), i.path.clone()))
            .collect();
        for (id, path) in candidates {
            let hash = match self.pixel_hashes.get(&id) {
                Some(h) => *h,
                None => {
                    let Ok(img) = image::open(&path) else { continue };
                    let img = img.into_rgba8();
                    let h = pixel_hash(img.width(), img.height(), img.as_raw());
                    self.pixel_hashes.insert(id.clone(), h);
                    h
                }
            };
            if hash == enc.pixels {
                return Some(id);
            }
        }
        None
    }

    /// Writes an encoded screenshot to disk and puts it on top of the shelf.
    /// The same picture already on the shelf is replaced: only the newest
    /// copy stays (in the old one's group, if it had one).
    pub fn insert(&mut self, enc: Encoded, now_ms: u64) -> Result<ShelfItem, String> {
        let same = self.find_same(&enc);
        let mut stem = format!("{PREFIX}{now_ms:013}");
        let mut n = 1;
        while self.paths(&stem).0.exists() {
            stem = format!("{PREFIX}{now_ms:013}-{n}");
            n += 1;
        }
        let (full, thumb_path) = self.paths(&stem);
        fs::write(&full, &enc.png).map_err(|e| e.to_string())?;
        fs::write(&thumb_path, &enc.thumb).map_err(|e| e.to_string())?;
        self.last_hash = Some(enc.hash);
        let item = ShelfItem {
            id: stem,
            path: full.to_string_lossy().to_string(),
            thumb_path: thumb_path.to_string_lossy().to_string(),
            thumb: data_url(&enc.thumb),
            width: enc.width,
            height: enc.height,
            created_at: now_ms,
            group: None,
        };
        self.pixel_hashes.insert(item.id.clone(), enc.pixels);
        self.items.insert(0, item.clone());
        let mut item = item;
        if let Some(old) = same {
            if let Some(group) = self.groups.get(&old).cloned() {
                self.groups.insert(item.id.clone(), group.clone());
                item.group = Some(group);
                self.sync_group_fields();
            }
            self.remove(&old);
        }
        self.enforce_limit();
        Ok(item)
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
        self.pixel_hashes.remove(id);
        if self.groups.remove(id).is_some() {
            self.dissolve_singletons();
            self.sync_group_fields();
        }
        true
    }

    /// Deletes the loose screenshots; groups are kept (they were made on purpose).
    /// With `everything` the groups go too.
    pub fn clear(&mut self, everything: bool) {
        let (gone, kept): (Vec<_>, Vec<_>) =
            std::mem::take(&mut self.items).into_iter().partition(|i| everything || i.group.is_none());
        for item in gone {
            let _ = fs::remove_file(&item.path);
            let _ = fs::remove_file(&item.thumb_path);
        }
        self.items = kept;
        if everything {
            self.groups.clear();
            self.save_groups();
        }
    }

    /// The ring: at most `limit` loose screenshots (oldest deleted first);
    /// grouped ones are kept up to `MAX_GROUPED`.
    fn enforce_limit(&mut self) {
        let mut loose = 0;
        let mut grouped = 0;
        let mut drop = Vec::new();
        for item in &self.items {
            let count = if item.group.is_some() { &mut grouped } else { &mut loose };
            *count += 1;
            if (item.group.is_none() && loose > self.limit) || (item.group.is_some() && grouped > MAX_GROUPED) {
                drop.push(item.id.clone());
            }
        }
        for id in drop {
            self.remove(&id);
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
        shelf.clear(true);
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

    #[test]
    fn groups_are_kept_by_ring_and_survive_reload() {
        let dir = temp_dir("groups");
        let mut shelf = Shelf::open(&dir, 2).unwrap();
        let ids: Vec<String> =
            (1..=3).map(|i| shelf.add_rgba(4, 4, pixels(4, 4, i), i as u64).unwrap().unwrap().id).collect();
        // ids[0] fell out of the ring of 2.
        assert_eq!(shelf.items().len(), 2);
        let gid = shelf.group(&[ids[1].clone()], &ids[2], 100).unwrap();
        assert_eq!(shelf.group_items(&gid).len(), 2);

        // Grouped screenshots do not count against the ring.
        for i in 4..=6 {
            shelf.add_rgba(4, 4, pixels(4, 4, i), i as u64).unwrap();
        }
        assert_eq!(shelf.items().len(), 4);
        assert_eq!(shelf.group_items(&gid).len(), 2);

        let reopened = Shelf::open(&dir, 2).unwrap();
        assert_eq!(reopened.group_items(&gid).len(), 2);
        assert_eq!(reopened.get(&ids[1]).unwrap().group.as_deref(), Some(gid.as_str()));

        // Clearing keeps groups unless asked.
        let mut shelf = reopened;
        shelf.clear(false);
        assert_eq!(shelf.items().len(), 2);
        shelf.clear(true);
        assert!(shelf.items().is_empty());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn merge_ungroup_and_dissolve() {
        let dir = temp_dir("merge");
        let mut shelf = Shelf::open(&dir, 10).unwrap();
        let ids: Vec<String> =
            (1..=5).map(|i| shelf.add_rgba(4, 4, pixels(4, 4, i), i as u64).unwrap().unwrap().id).collect();
        let a = shelf.group(&[ids[0].clone()], &ids[1], 10).unwrap();
        let b = shelf.group(&[ids[2].clone()], &ids[3], 11).unwrap();
        assert_ne!(a, b);
        // Drop group b onto a group member of a: all four end up in a.
        assert_eq!(shelf.group(&[b.clone()], &ids[0], 12).unwrap(), a);
        assert_eq!(shelf.group_items(&a).len(), 4);
        assert!(shelf.group_items(&b).is_empty());
        // Drop a loose one onto the group id itself.
        shelf.group(&[ids[4].clone()], &a, 13).unwrap();
        assert_eq!(shelf.group_items(&a).len(), 5);

        for id in &ids[..3] {
            assert!(shelf.ungroup(id));
        }
        assert_eq!(shelf.group_items(&a).len(), 2);
        // Removing one of the last two dissolves the group.
        assert!(shelf.remove(&ids[3]));
        assert!(shelf.group_items(&a).is_empty());
        assert!(shelf.get(&ids[4]).unwrap().group.is_none());
        assert!(shelf.group(&[ids[4].clone()], "nope", 14).is_err());

        let gid = shelf.group(&[ids[0].clone()], &ids[1], 15).unwrap();
        assert_eq!(shelf.remove_group(&gid), 2);
        assert_eq!(shelf.items().len(), 2);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn edited_png_is_stored_verbatim() {
        let dir = temp_dir("edited");
        let mut shelf = Shelf::open(&dir, 5).unwrap();
        let enc = encode(300, 100, pixels(300, 100, 9)).unwrap();
        let png = enc.png.clone();
        let again = encode_png(&png).unwrap();
        assert_eq!((again.width, again.height), (300, 100));
        assert_eq!(again.hash, enc.hash);
        let item = shelf.insert(again, 50).unwrap();
        assert_eq!(fs::read(&item.path).unwrap(), png);
        assert!(encode_png(b"not a png").is_err());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn same_picture_replaces_the_old_copy_and_keeps_its_group() {
        let dir = temp_dir("dedupe");
        let mut shelf = Shelf::open(&dir, 10).unwrap();
        let a = shelf.add_rgba(6, 4, pixels(6, 4, 1), 1).unwrap().unwrap();
        let b = shelf.add_rgba(6, 4, pixels(6, 4, 2), 2).unwrap().unwrap();
        let gid = shelf.group(&[a.id.clone()], &b.id, 3).unwrap();
        shelf.add_rgba(6, 4, pixels(6, 4, 3), 4).unwrap();
        // The same pixels as `a` again (e.g. copied once more, or saved from
        // the editor with another PNG encoder).
        let png = encode(6, 4, pixels(6, 4, 1)).unwrap().png;
        let again = shelf.insert(encode_png(&png).unwrap(), 5).unwrap();
        assert!(shelf.get(&a.id).is_none(), "old copy removed");
        assert!(!Path::new(&a.path).exists());
        assert_eq!(shelf.items()[0].id, again.id, "new copy on top");
        assert_eq!(again.group.as_deref(), Some(gid.as_str()));
        assert_eq!(shelf.group_items(&gid).len(), 2);
        assert_eq!(shelf.items().len(), 3);
        // After a restart (empty hash cache) duplicates are still found.
        let mut reopened = Shelf::open(&dir, 10).unwrap();
        reopened.insert(encode(6, 4, pixels(6, 4, 3)).unwrap(), 6).unwrap();
        assert_eq!(reopened.items().len(), 3);
        // Same size, different pixels: kept.
        reopened.insert(encode(6, 4, pixels(6, 4, 9)).unwrap(), 7).unwrap();
        assert_eq!(reopened.items().len(), 4);
        fs::remove_dir_all(&dir).unwrap();
    }
}
