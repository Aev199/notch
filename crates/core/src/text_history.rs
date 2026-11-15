//! In-memory ring of recently copied text fragments.

use serde::{Deserialize, Serialize};

/// Longest fragment we keep, in characters. Bigger copies are truncated so a
/// pasted log file cannot blow up memory or the UI.
pub const MAX_ENTRY_CHARS: usize = 20_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextEntry {
    pub id: u64,
    pub text: String,
    /// Unix time in milliseconds.
    pub copied_at: u64,
    /// Pinned entries are never pushed out by the ring or removed by
    /// "clear", and are kept on disk.
    #[serde(default)]
    pub pinned: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TextHistory {
    entries: Vec<TextEntry>,
    next_id: u64,
    #[serde(skip)]
    limit: usize,
}

/// The cleaned form of a copied fragment: trims the ends, strips trailing
/// spaces on every line, collapses runs of spaces/tabs inside a line into one
/// space and keeps at most one empty line in a row. Line breaks themselves are
/// preserved.
///
/// The history does NOT store this form: indentation and tabs matter when the
/// text is pasted back (code, YAML, spreadsheet cells). It is the key that
/// tells whether two copies are "the same text", and the form handed to the
/// translator.
pub fn normalize(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut blank_run = 0usize;
    for line in raw.replace("\r\n", "\n").replace('\r', "\n").split('\n') {
        let mut cleaned = String::with_capacity(line.len());
        let mut prev_space = false;
        for ch in line.chars() {
            let is_space = ch == ' ' || ch == '\t' || ch == '\u{a0}';
            if is_space {
                if !prev_space {
                    cleaned.push(' ');
                }
                prev_space = true;
            } else if !ch.is_control() {
                cleaned.push(ch);
                prev_space = false;
            }
        }
        let cleaned = cleaned.trim();
        if cleaned.is_empty() {
            blank_run += 1;
            if blank_run > 1 {
                continue;
            }
        } else {
            blank_run = 0;
        }
        out.push_str(cleaned);
        out.push('\n');
    }
    let trimmed = out.trim();
    match trimmed.char_indices().nth(MAX_ENTRY_CHARS) {
        Some((cut, _)) => trimmed[..cut].to_string(),
        None => trimmed.to_string(),
    }
}

impl TextHistory {
    pub fn new(limit: usize) -> Self {
        Self { entries: Vec::new(), next_id: 1, limit: limit.max(1) }
    }

    pub fn set_limit(&mut self, limit: usize) {
        self.limit = limit.max(1);
        self.enforce_limit();
    }

    pub fn entries(&self) -> &[TextEntry] {
        &self.entries
    }

    pub fn latest(&self) -> Option<&TextEntry> {
        self.entries.first()
    }

    /// Adds a fragment to the top, exactly as it was copied (only cut to
    /// [`MAX_ENTRY_CHARS`]). Texts that differ only in spacing count as the
    /// same one: the entry moves to the top and takes the newest spelling
    /// instead of being stored twice. Returns `false` when nothing changed
    /// (blank text, or exactly the newest entry again).
    pub fn push(&mut self, raw: &str, now_ms: u64) -> bool {
        let key = normalize(raw);
        if key.is_empty() {
            return false;
        }
        let text = match raw.char_indices().nth(MAX_ENTRY_CHARS) {
            Some((cut, _)) => raw[..cut].to_string(),
            None => raw.to_string(),
        };
        if let Some(pos) = self.entries.iter().position(|e| normalize(&e.text) == key) {
            if pos == 0 && self.entries[0].text == text {
                return false;
            }
            let mut entry = self.entries.remove(pos);
            // The newest entry keeps its time: only its spelling changed.
            if pos != 0 {
                entry.copied_at = now_ms;
            }
            entry.text = text;
            self.entries.insert(0, entry);
            return true;
        }
        let id = self.next_id.max(1);
        self.next_id = id + 1;
        self.entries.insert(0, TextEntry { id, text, copied_at: now_ms, pinned: false });
        self.enforce_limit();
        true
    }

    /// At most `limit` unpinned entries; pinned ones do not count.
    fn enforce_limit(&mut self) {
        let mut unpinned = 0;
        let limit = self.limit;
        self.entries.retain(|e| {
            if e.pinned {
                return true;
            }
            unpinned += 1;
            unpinned <= limit
        });
    }

    pub fn set_pinned(&mut self, id: u64, pinned: bool) -> bool {
        let Some(entry) = self.entries.iter_mut().find(|e| e.id == id) else { return false };
        entry.pinned = pinned;
        self.enforce_limit();
        true
    }

    pub fn pinned(&self) -> impl Iterator<Item = &TextEntry> {
        self.entries.iter().filter(|e| e.pinned)
    }

    pub fn remove(&mut self, id: u64) -> bool {
        let before = self.entries.len();
        self.entries.retain(|e| e.id != id);
        before != self.entries.len()
    }

    pub fn get(&self, id: u64) -> Option<&TextEntry> {
        self.entries.iter().find(|e| e.id == id)
    }

    /// Removes everything except pinned entries.
    pub fn clear(&mut self) {
        self.entries.retain(|e| e.pinned);
    }

    /// Restores a history loaded from disk, re-applying the limit. Entries
    /// with the same text are kept once (pinned wins).
    pub fn restore(entries: Vec<TextEntry>, limit: usize) -> Self {
        let mut history = Self::new(limit);
        history.next_id = entries.iter().map(|e| e.id).max().unwrap_or(0) + 1;
        for entry in entries {
            let key = normalize(&entry.text);
            match history.entries.iter_mut().find(|e| normalize(&e.text) == key) {
                Some(existing) => existing.pinned |= entry.pinned,
                None => history.entries.push(entry),
            }
        }
        history.entries.sort_by_key(|e| std::cmp::Reverse(e.copied_at));
        history.enforce_limit();
        history
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_collapses_spaces_and_blank_lines() {
        let raw = "  Hello\t\t world  \r\n\r\n\r\n\r\n  second   line \n";
        assert_eq!(normalize(raw), "Hello world\n\nsecond line");
    }

    #[test]
    fn normalize_drops_control_chars_and_nbsp() {
        assert_eq!(normalize("a\u{a0}\u{a0}b\u{7}c"), "a bc");
    }

    #[test]
    fn normalize_truncates_huge_input() {
        let raw = "я".repeat(MAX_ENTRY_CHARS + 50);
        assert_eq!(normalize(&raw).chars().count(), MAX_ENTRY_CHARS);
    }

    #[test]
    fn push_skips_empty_and_repeated_top() {
        let mut h = TextHistory::new(5);
        assert!(!h.push("   \n\t", 1));
        assert!(h.push("one", 1));
        assert!(!h.push("one", 2));
        assert_eq!(h.entries().len(), 1);
    }

    #[test]
    fn keeps_the_text_as_copied() {
        let mut h = TextHistory::new(5);
        let code = "def f():\r\n\tif x:\r\n\t\treturn  1\r\n";
        assert!(h.push(code, 1));
        assert_eq!(h.entries()[0].text, code, "indentation, tabs and line ends survive");
        // Cells copied from a spreadsheet keep their tabs.
        assert!(h.push("a\tb\tc", 2));
        assert_eq!(h.entries()[0].text, "a\tb\tc");
        // The same text with other spacing is one entry with the newest spelling.
        let id = h.entries()[0].id;
        assert!(h.push("  a b  c ", 3));
        assert_eq!(h.entries().len(), 2);
        assert_eq!((h.entries()[0].id, h.entries()[0].text.as_str(), h.entries()[0].copied_at), (id, "  a b  c ", 2));
        assert!(h.push("def f():\n if x:\n  return 1", 4));
        assert_eq!(h.entries().len(), 2);
        assert_eq!(h.entries()[0].copied_at, 4);
        let long = "я".repeat(MAX_ENTRY_CHARS + 50);
        h.push(&long, 5);
        assert_eq!(h.entries()[0].text.chars().count(), MAX_ENTRY_CHARS);
    }

    #[test]
    fn duplicate_moves_to_top_and_keeps_id() {
        let mut h = TextHistory::new(5);
        h.push("a", 1);
        h.push("b", 2);
        let id_a = h.entries()[1].id;
        assert!(h.push("a", 3));
        assert_eq!(h.entries()[0].text, "a");
        assert_eq!(h.entries()[0].id, id_a);
        assert_eq!(h.entries()[0].copied_at, 3);
        assert_eq!(h.entries().len(), 2);
    }

    #[test]
    fn ring_drops_oldest() {
        let mut h = TextHistory::new(3);
        for (i, t) in ["1", "2", "3", "4"].iter().enumerate() {
            h.push(t, i as u64);
        }
        let texts: Vec<_> = h.entries().iter().map(|e| e.text.as_str()).collect();
        assert_eq!(texts, ["4", "3", "2"]);
        h.set_limit(1);
        assert_eq!(h.entries().len(), 1);
    }

    #[test]
    fn restore_continues_ids() {
        let mut h = TextHistory::new(5);
        h.push("x", 1);
        h.push("y", 2);
        let mut r = TextHistory::restore(h.entries().to_vec(), 5);
        r.push("z", 3);
        assert_eq!(r.entries()[0].id, 3);
        assert!(r.remove(1));
        assert!(r.get(1).is_none());
    }

    #[test]
    fn pinned_entries_survive_ring_and_clear() {
        let mut h = TextHistory::new(2);
        h.push("keep", 1);
        let id = h.entries()[0].id;
        assert!(h.set_pinned(id, true));
        for (i, t) in ["a", "b", "c"].iter().enumerate() {
            h.push(t, 10 + i as u64);
        }
        let texts: Vec<_> = h.entries().iter().map(|e| e.text.as_str()).collect();
        assert_eq!(texts, ["c", "b", "keep"]);
        h.clear();
        assert_eq!(h.entries().len(), 1);
        assert_eq!(h.pinned().count(), 1);
        // Copying a pinned text again keeps it pinned.
        h.push("x", 20);
        h.push("keep", 21);
        assert!(h.entries()[0].pinned);
        assert!(h.set_pinned(id, false));
        assert!(!h.set_pinned(999, true));
    }

    #[test]
    fn restore_merges_pins_with_history() {
        let mk = |id, text: &str, at, pinned| TextEntry { id, text: text.into(), copied_at: at, pinned };
        let h = TextHistory::restore(vec![mk(1, "a", 5, false), mk(2, "b", 9, false), mk(3, "a", 1, true)], 5);
        let view: Vec<_> = h.entries().iter().map(|e| (e.text.as_str(), e.pinned)).collect();
        assert_eq!(view, [("b", false), ("a", true)]);
    }
}
