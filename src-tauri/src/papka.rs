//! «В проект…»: entries for Papka's project inboxes (see core `papka`).

use std::path::PathBuf;

use notch_core::papka::{self, Payload, Project, Source};
use serde::Serialize;
use tauri::{AppHandle, State};

use crate::capture;
use crate::state::{lock, Shared};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectList {
    pub projects: Vec<ProjectView>,
    /// The project chosen last time, if it is still there.
    pub last: Option<String>,
}

#[derive(Serialize)]
pub struct ProjectView {
    pub id: String,
    pub name: String,
    pub inbox: String,
}

fn registry() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    papka::registry_path(&base)
}

fn project(id: &str) -> Result<Project, String> {
    papka::projects(&registry())?
        .into_iter()
        .find(|p| p.id == id)
        .ok_or_else(|| "Проекта больше нет в списке Papka: выберите другой".to_string())
}

/// Writes the entry and remembers the project for next time.
fn send(state: &Shared, id: &str, payload: Payload<'_>, caption: &str, source: &Source) -> Result<String, String> {
    let project = project(id)?;
    papka::send(&project, payload, caption, source, papka::local_now())?;
    papka::remember_project(&state.data_dir, id);
    Ok(project.name)
}

/// Papka's projects for the «В проект…» list.
#[tauri::command]
pub fn papka_projects(state: State<'_, Shared>) -> Result<ProjectList, String> {
    let projects = papka::projects(&registry())?;
    let last = papka::last_project(&state.data_dir).filter(|id| projects.iter().any(|p| &p.id == id));
    Ok(ProjectList {
        projects: projects
            .into_iter()
            .map(|p| ProjectView { inbox: p.inbox.display().to_string(), id: p.id, name: p.name })
            .collect(),
        last,
    })
}

/// Percent-decoding of a header value (`encodeURIComponent` on the page):
/// headers carry ASCII only, captions are Russian.
fn decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(byte) = u8::from_str_radix(std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("zz"), 16) {
                out.push(byte);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The capture window's picture (raw PNG). Headers: `x-papka-project` and
/// `x-papka-caption` (URI-encoded). The window closes once it is written.
#[tauri::command]
pub async fn papka_send_capture(request: tauri::ipc::Request<'_>, app: AppHandle, state: State<'_, Shared>) -> Result<String, String> {
    let tauri::ipc::InvokeBody::Raw(png) = request.body() else {
        return Err("окно снимка прислало не картинку".into());
    };
    let header = |name: &str| request.headers().get(name).and_then(|v| v.to_str().ok()).map(decode).unwrap_or_default();
    let source = lock(&state.capture_source).clone();
    let name = send(&state, &header("x-papka-project"), Payload::Image(png), &header("x-papka-caption"), &source)?;
    capture::capture_close(app).await?;
    Ok(name)
}

/// A screenshot from the shelf.
#[tauri::command]
pub async fn papka_send_shelf(id: String, project: String, caption: String, state: State<'_, Shared>) -> Result<String, String> {
    let path = lock(&state.shelf).get(&id).map(|i| i.path.clone()).ok_or("скриншот не найден")?;
    let png = std::fs::read(&path).map_err(|e| e.to_string())?;
    send(&state, &project, Payload::Image(&png), &caption, &Source { app: "notch.exe".into(), window: "Полка снимков".into() })
}

/// A text from the clipboard history (or any text). Async like the others:
/// the inbox is often on a network drive, and a slow one must not freeze
/// the main thread.
#[tauri::command]
pub async fn papka_send_text(text: String, project: String, caption: String, state: State<'_, Shared>) -> Result<String, String> {
    if text.trim().is_empty() {
        return Err("пустой текст".into());
    }
    send(&state, &project, Payload::Text(&text), &caption, &Source { app: "notch.exe".into(), window: "Буфер".into() })
}

/// A file or folder from the file shelf: only its path goes.
#[tauri::command]
pub async fn papka_send_file(id: u64, project: String, caption: String, state: State<'_, Shared>) -> Result<String, String> {
    let path = lock(&state.files).get(id).map(|e| PathBuf::from(&e.path)).ok_or("файла нет на полке")?;
    send(&state, &project, Payload::File(&path), &caption, &Source { app: "notch.exe".into(), window: "Полка файлов".into() })
}

#[cfg(test)]
mod tests {
    use super::decode;

    #[test]
    fn captions_come_through_headers_whole() {
        assert_eq!(decode("%D0%A1%D0%B2%D0%B0%D0%B8%20%D0%A1-18"), "Сваи С-18");
        assert_eq!(decode("p-0123"), "p-0123");
        assert_eq!(decode("100%"), "100%");
        assert_eq!(decode("%zz"), "%zz");
    }
}
