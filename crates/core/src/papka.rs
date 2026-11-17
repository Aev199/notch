//! «В проект…»: sends a screenshot, a text or a file link into a project
//! of Papka, the file manager.
//!
//! Papka lists its projects in `%LOCALAPPDATA%\Papka\projects.json`; each
//! has an inbox folder. An entry is a picture (if any) and a `.json` beside
//! it, written in that order, each through a temporary file and a rename,
//! so Papka never reads half an entry. Papka need not be running. The
//! format is described in Papka's `docs/INBOX.md`; the two programs share
//! nothing else.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

pub const REGISTRY_FILE: &str = "projects.json";
/// The project chosen last, beside Notch's other files.
pub const LAST_FILE: &str = "notch.papka.json";
const FORMAT: u32 = 1;
const MAX_CAPTION: usize = 500;
const MAX_TEXT: usize = 100_000;
/// A file name keeps this much of the caption.
const MAX_SLUG: usize = 60;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Project {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root: Option<PathBuf>,
    pub inbox: PathBuf,
}

#[derive(Deserialize)]
struct Registry {
    /// Read entry by entry: one odd entry (no inbox yet, an older format)
    /// must not hide the others.
    #[serde(default)]
    projects: Vec<serde_json::Value>,
}

/// Where Papka lists its projects, given `%LOCALAPPDATA%`.
pub fn registry_path(local_app_data: &Path) -> PathBuf {
    local_app_data.join("Papka").join(REGISTRY_FILE)
}

/// Papka's projects, in its order. A missing list means Papka has not
/// written one yet (it is not installed, or has no projects).
pub fn projects(registry: &Path) -> Result<Vec<Project>, String> {
    let raw = match fs::read_to_string(registry) {
        Ok(raw) => raw,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err("Список проектов Papka не найден: запустите Papka и создайте проект".into())
        }
        Err(e) => return Err(format!("Список проектов Papka не читается: {e}")),
    };
    let registry: Registry = serde_json::from_str(raw.trim_start_matches('\u{feff}'))
        .map_err(|e| format!("Список проектов Papka повреждён: {e}"))?;
    Ok(registry
        .projects
        .into_iter()
        .filter_map(|entry| serde_json::from_value::<Project>(entry).ok())
        .filter(|p| !p.id.is_empty() && !p.name.is_empty())
        .collect())
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Source {
    /// Program file name, e.g. `acad.exe`.
    #[serde(default)]
    pub app: String,
    /// Title of its window, e.g. the drawing's name.
    #[serde(default)]
    pub window: String,
}

/// What is sent.
pub enum Payload<'a> {
    /// A PNG picture.
    Image(&'a [u8]),
    Text(&'a str),
    /// A link to a file or folder; the file itself stays where it is.
    File(&'a Path),
}

#[derive(Serialize)]
struct Sidecar<'a> {
    format: u32,
    kind: &'static str,
    created: String,
    caption: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    image: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    file: Option<&'a Path>,
    source: &'a Source,
    project: &'a str,
}

/// A caption made safe for a file name: no `\ / : * ? " < > |`, no control
/// characters, no trailing dots or spaces (Windows drops them), short.
pub fn slug(caption: &str) -> String {
    let cleaned: String = caption
        .chars()
        .map(|c| if c.is_control() || r#"\/:*?"<>|"#.contains(c) { ' ' } else { c })
        .collect();
    let words = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    let short: String = words.chars().take(MAX_SLUG).collect();
    short.trim_end_matches(['.', ' ']).to_string()
}

fn write_atomic(path: &Path, data: &[u8]) -> Result<(), String> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    fs::write(&tmp, data).map_err(|e| format!("{}: {e}", tmp.display()))?;
    fs::rename(&tmp, path).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        format!("{}: {e}", path.display())
    })
}

/// Writes one entry into the project's inbox and returns its `.json`.
/// `now` is the local time with its offset.
pub fn send(
    project: &Project,
    payload: Payload<'_>,
    caption: &str,
    source: &Source,
    now: chrono::DateTime<chrono::FixedOffset>,
) -> Result<PathBuf, String> {
    let caption: String = caption.trim().chars().take(MAX_CAPTION).collect();
    let (kind, fallback) = match &payload {
        Payload::Image(_) => ("image", "Снимок".to_string()),
        Payload::Text(text) => ("text", text.lines().find(|l| !l.trim().is_empty()).unwrap_or("Текст").to_string()),
        Payload::File(path) => ("file", path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "Файл".into())),
    };
    let mut name = slug(if caption.is_empty() { &fallback } else { &caption });
    if name.is_empty() {
        name = "Запись".into();
    }
    fs::create_dir_all(&project.inbox)
        .map_err(|e| format!("Папка входящих проекта «{}» недоступна: {e}", project.name))?;
    // Names start with a sortable stamp; a second entry in the same second
    // gets " (2)".
    let base = format!("{} {name}", now.format("%Y%m%d-%H%M%S"));
    let stem = (1..1000)
        .map(|n| if n == 1 { base.clone() } else { format!("{base} ({n})") })
        .find(|stem| {
            !project.inbox.join(format!("{stem}.json")).exists() && !project.inbox.join(format!("{stem}.png")).exists()
        })
        .ok_or("слишком много записей с одним именем")?;
    let image = match &payload {
        Payload::Image(png) => {
            let file = format!("{stem}.png");
            write_atomic(&project.inbox.join(&file), png)?;
            Some(file)
        }
        _ => None,
    };
    let text = match &payload {
        Payload::Text(text) => Some(if text.len() > MAX_TEXT { &text[..text.floor_char_boundary(MAX_TEXT)] } else { text }),
        _ => None,
    };
    let sidecar = Sidecar {
        format: FORMAT,
        kind,
        created: now.to_rfc3339_opts(chrono::SecondsFormat::Secs, false),
        caption: &caption,
        text,
        image,
        file: match &payload {
            Payload::File(path) => Some(path),
            _ => None,
        },
        source,
        project: &project.id,
    };
    let json = serde_json::to_string_pretty(&sidecar).map_err(|e| e.to_string())?;
    let path = project.inbox.join(format!("{stem}.json"));
    write_atomic(&path, json.as_bytes())?;
    Ok(path)
}

/// The local time with its offset, as `send` wants it.
pub fn local_now() -> chrono::DateTime<chrono::FixedOffset> {
    chrono::Local::now().fixed_offset()
}

/// The project chosen last, to offer it first next time.
pub fn last_project(data_dir: &Path) -> Option<String> {
    #[derive(Deserialize)]
    struct Last {
        last: String,
    }
    let raw = fs::read_to_string(data_dir.join(LAST_FILE)).ok()?;
    serde_json::from_str::<Last>(&raw).ok().map(|l| l.last).filter(|id| !id.is_empty())
}

pub fn remember_project(data_dir: &Path, id: &str) {
    let json = serde_json::json!({ "last": id }).to_string();
    let _ = write_atomic(&data_dir.join(LAST_FILE), json.as_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("notch-papka-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn at(time: &str) -> chrono::DateTime<chrono::FixedOffset> {
        chrono::DateTime::parse_from_rfc3339(time).unwrap()
    }

    #[test]
    fn projects_are_read_from_papkas_list() {
        let base = dir("registry");
        let path = registry_path(&base);
        assert!(projects(&path).unwrap_err().contains("запустите Papka"));
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "\u{feff}{\"format\":1,\"program\":\"Papka\",\"projects\":[{\"id\":\"p-1\",\"name\":\"Корпус 2\",\"root\":\"Z:/044\",\"inbox\":\"Z:/044/_Входящие\"},{\"id\":\"\",\"name\":\"x\",\"inbox\":\"y\"}]}").unwrap();
        let list = projects(&path).unwrap();
        assert_eq!(list.len(), 1, "an entry without a number is left out");
        // Entries without an inbox or with a null one are skipped; the rest stay.
        fs::write(&path, r#"{"projects":[{"id":"p-0","name":"Новый"},{"id":"p-9","name":"Старый","inbox":null},{"id":"p-1","name":"Корпус 2","inbox":"Z:/044/_Входящие"}]}"#).unwrap();
        assert_eq!(projects(&path).unwrap().iter().map(|p| p.id.as_str()).collect::<Vec<_>>(), ["p-1"]);
        fs::write(&path, "\u{feff}{\"format\":1,\"program\":\"Papka\",\"projects\":[{\"id\":\"p-1\",\"name\":\"Корпус 2\",\"root\":\"Z:/044\",\"inbox\":\"Z:/044/_Входящие\"},{\"id\":\"\",\"name\":\"x\",\"inbox\":\"y\"}]}").unwrap();
        let list = projects(&path).unwrap();
        assert_eq!((list[0].name.as_str(), list[0].inbox.as_path()), ("Корпус 2", Path::new("Z:/044/_Входящие")));
        fs::write(&path, "{ broken").unwrap();
        assert!(projects(&path).unwrap_err().contains("повреждён"));
    }

    #[test]
    fn captions_become_safe_file_names() {
        assert_eq!(slug("Сверить привязку С-18: ось Б/4?"), "Сверить привязку С-18 ось Б 4");
        assert_eq!(slug("  конец с точкой... "), "конец с точкой");
        assert_eq!(slug("a\nb\tc"), "a b c");
        assert_eq!(slug(&"я".repeat(100)).chars().count(), MAX_SLUG);
        assert_eq!(slug("<>|"), "");
    }

    #[test]
    fn an_entry_is_a_picture_and_its_json() {
        let base = dir("send");
        let project = Project { id: "p-1".into(), name: "Корпус 2".into(), root: None, inbox: base.join("Корпус 2").join("_Входящие") };
        let source = Source { app: "acad.exe".into(), window: "Свайное поле.dwg".into() };
        let now = at("2026-11-17T14:32:05+03:00");
        let first = send(&project, Payload::Image(b"\x89PNG"), "Сверить привязку С-18", &source, now).unwrap();
        let second = send(&project, Payload::Image(b"\x89PNG"), "Сверить привязку С-18", &source, now).unwrap();
        assert!(first.ends_with("20261117-143205 Сверить привязку С-18.json"));
        assert!(second.ends_with("20261117-143205 Сверить привязку С-18 (2).json"), "same second, same caption");
        let json: serde_json::Value = serde_json::from_str(&fs::read_to_string(&first).unwrap()).unwrap();
        assert_eq!(json["kind"], "image");
        assert_eq!(json["image"], "20261117-143205 Сверить привязку С-18.png");
        assert_eq!(json["created"], "2026-11-17T14:32:05+03:00");
        assert_eq!((json["source"]["app"].as_str(), json["project"].as_str()), (Some("acad.exe"), Some("p-1")));
        assert_eq!(fs::read(project.inbox.join("20261117-143205 Сверить привязку С-18.png")).unwrap(), b"\x89PNG");
        let leftovers = fs::read_dir(&project.inbox).unwrap().filter(|e| e.as_ref().unwrap().path().to_string_lossy().ends_with(".tmp")).count();
        assert_eq!(leftovers, 0);
    }

    #[test]
    fn texts_and_file_links_need_no_picture() {
        let base = dir("kinds");
        let project = Project { id: "p-2".into(), name: "Остров".into(), root: None, inbox: base.join("in") };
        let none = Source::default();
        let now = at("2026-11-17T15:00:00+03:00");
        let text = send(&project, Payload::Text("\n Позвонить ГИПу\nпо узлу Б/4"), "", &none, now).unwrap();
        assert!(text.ends_with("20261117-150000 Позвонить ГИПу.json"), "no caption: the first line names it");
        let json: serde_json::Value = serde_json::from_str(&fs::read_to_string(&text).unwrap()).unwrap();
        assert_eq!((json["kind"].as_str(), json["text"].as_str(), json.get("image")), (Some("text"), Some("\n Позвонить ГИПу\nпо узлу Б/4"), None));
        let file = send(&project, Payload::File(Path::new("Z:/044/Сваи.dwg")), "", &none, now).unwrap();
        let json: serde_json::Value = serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!((json["kind"].as_str(), json["file"].as_str()), (Some("file"), Some("Z:/044/Сваи.dwg")));
        assert!(file.ends_with("20261117-150000 Сваи.dwg.json"));
    }

    #[test]
    fn an_unreachable_inbox_is_reported_by_name() {
        let base = dir("blocked");
        fs::write(base.join("file"), b"x").unwrap();
        let project = Project { id: "p-3".into(), name: "Сеть".into(), root: None, inbox: base.join("file").join("in") };
        let error = send(&project, Payload::Text("x"), "", &Source::default(), at("2026-11-17T15:00:00+03:00")).unwrap_err();
        assert!(error.contains("«Сеть»"), "{error}");
    }

    #[test]
    fn the_last_project_is_remembered() {
        let base = dir("last");
        assert_eq!(last_project(&base), None);
        remember_project(&base, "p-7");
        assert_eq!(last_project(&base).as_deref(), Some("p-7"));
    }
}
