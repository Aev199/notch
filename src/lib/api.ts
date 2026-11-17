import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type Tab = "shelf" | "files" | "clipboard" | "translate" | "settings";
export type Provider = "offline" | "google" | "deepl" | "llm";
export type Mode = "open" | "compact" | "thin";
export type Lang = "ru" | "en";

export interface TextEntry {
  id: number;
  text: string;
  copied_at: number;
  pinned: boolean;
}

export interface OcrText {
  text: string;
  language: string;
}

export interface ShelfItem {
  id: string;
  path: string;
  thumb_path: string;
  thumb: string;
  width: number;
  height: number;
  created_at: number;
  group: string | null;
}

/** A file or folder on the file shelf (only its path is kept). */
export interface FileEntry {
  id: number;
  path: string;
  name: string;
  addedAt: number;
  isDir: boolean;
  size: number;
  /** Moved or deleted since it was put here. */
  missing: boolean;
}

/** A project of Papka, the file manager, for «В проект…». */
export interface PapkaProject {
  id: string;
  name: string;
  inbox: string;
}

export interface PapkaProjects {
  projects: PapkaProject[];
  /** Chosen last time, if still there. */
  last: string | null;
}

export interface Settings {
  provider: Provider;
  deeplKey: string;
  llmUrl: string;
  llmModel: string;
  llmKey: string;
  hotkey: string;
  doubleCopyTranslate: boolean;
  shelfLimit: number;
  textLimit: number;
  persistTextHistory: boolean;
  hideInFullscreen: boolean;
  captureImages: boolean;
  idleThin: boolean;
  idleSeconds: number;
  autoUpdate: boolean;
  lastUpdateCheck: number;
  capturePrintscreen: boolean;
  historyHotkey: string;
  filesShelf: boolean;
  followCursor: boolean;
  animations: boolean;
}

export interface UpdateOffer {
  version: string;
  notes: string[];
  sha256: string;
  source: string;
  page: string;
}

export type UpdateStatus =
  | { state: "idle" }
  | { state: "checking" }
  | { state: "upToDate"; checked_at: number }
  | { state: "available"; update: UpdateOffer; from_file: boolean }
  | { state: "downloading"; done: number; total: number | null }
  | { state: "ready"; version: string }
  | { state: "offline"; error: string }
  | { state: "watching"; until: number }
  | { state: "failed"; error: string };

export interface UpdateInfo {
  version: string;
  status: UpdateStatus;
  hasPrevious: boolean;
  browserUrl: string;
}

export interface Snapshot {
  settings: Settings;
  texts: TextEntry[];
  shelf: ShelfItem[];
  files: FileEntry[];
  /** Picture shown under the cursor while files are dragged out. */
  fileIcon: string;
  dataDir: string;
  autostart: boolean;
  mode: Mode;
}

export interface Translation {
  text: string;
  from: Lang;
  to: Lang;
  provider: Provider;
}

export interface IslandEvent {
  mode: Mode;
  tab: Tab | null;
  /** Put the caret into the tab's text field (translator, clipboard search). */
  focus: boolean;
}

export interface TranslateRequest {
  text: string;
  focus: boolean;
}

export const api = {
  snapshot: () => invoke<Snapshot>("snapshot"),
  setIslandSize: (width: number, height: number) => invoke("set_island_size", { width, height }),
  close: () => invoke("close_island"),
  open: (tab?: Tab) => invoke("open_island", { tab }),
  copyEntry: (id: number, close: boolean) => invoke("copy_entry", { id, close }),
  copyText: (text: string) => invoke("copy_text", { text }),
  removeEntry: (id: number) => invoke("remove_entry", { id }),
  clearEntries: () => invoke("clear_entries"),
  shelfCopy: (id: string) => invoke("shelf_copy", { id }),
  shelfRemove: (id: string) => invoke("shelf_remove", { id }),
  shelfClear: (everything = false) => invoke("shelf_clear", { everything }),
  shelfFromClipboard: () => invoke("shelf_from_clipboard"),
  shelfGroup: (ids: string[], target: string) => invoke<string>("shelf_group", { ids, target }),
  shelfUngroup: (id: string) => invoke("shelf_ungroup", { id }),
  shelfRemoveGroup: (group: string) => invoke("shelf_remove_group", { group }),
  pasteEntry: (id: number) => invoke("paste_entry", { id }),
  pinEntry: (id: number, pinned: boolean) => invoke("pin_entry", { id, pinned }),
  shelfImage: (id: string) => invoke<ArrayBuffer>("shelf_image", { id }),
  openEditor: (id: string) => invoke("open_editor", { id }),
  /** The editor has drawn its picture: show it and bring it forward. */
  editorShow: () => invoke("editor_show"),
  /** Raw PNG in, recognised text out (Windows OCR). */
  ocrPng: (png: Uint8Array) => invoke<OcrText>("ocr_png", png),
  pasteText: (text: string) => invoke("paste_text", { text }),
  openDataFolder: () => invoke("open_data_folder"),
  setDragging: (dragging: boolean) => invoke("set_dragging", { dragging }),
  beginInput: () => invoke("begin_input"),
  endInput: () => invoke("end_input"),
  translateOnline: (text: string) => invoke<Translation>("translate", { text }),
  clipboardText: () => invoke<string | null>("clipboard_text"),
  saveSettings: (settings: Settings) => invoke<Settings>("save_settings", { new: settings }),
  setAutostart: (enabled: boolean) => invoke<boolean>("set_autostart", { enabled }),
  quit: () => invoke("quit"),
  filesRemove: (id: number) => invoke("files_remove", { id }),
  filesClear: () => invoke("files_clear"),
  filesRefresh: () => invoke("files_refresh"),
  filesOpen: (id: number) => invoke("files_open", { id }),
  filesReveal: (id: number) => invoke("files_reveal", { id }),
  /** The frozen screen: 8 bytes of width and height (u32 LE), then RGBA. */
  captureImage: () => invoke<ArrayBuffer>("capture_image"),
  captureShow: () => invoke("capture_show"),
  captureClose: () => invoke("capture_close"),
  captureText: (text: string, translate: boolean) => invoke("capture_text", { text, translate }),
  papkaProjects: () => invoke<PapkaProjects>("papka_projects"),
  papkaSendShelf: (id: string, project: string, caption: string) => invoke<string>("papka_send_shelf", { id, project, caption }),
  papkaSendText: (text: string, project: string, caption: string) => invoke<string>("papka_send_text", { text, project, caption }),
  papkaSendFile: (id: number, project: string, caption: string) => invoke<string>("papka_send_file", { id, project, caption }),
  /** The capture's picture; headers carry ASCII only, so the caption is URI-encoded. */
  papkaSendCapture: (png: Uint8Array, project: string, caption: string) =>
    invoke<string>("papka_send_capture", png, {
      headers: { "x-papka-project": encodeURIComponent(project), "x-papka-caption": encodeURIComponent(caption) },
    }),
  updateInfo: () => invoke<UpdateInfo>("update_info"),
  updateCheck: () => invoke("update_check"),
  updateInstall: () => invoke("update_install"),
  updateBrowser: () => invoke("update_browser"),
  updateRestart: () => invoke("update_restart"),
  updateRollback: () => invoke("update_rollback"),
};

export function on<T>(event: string, handler: (payload: T) => void): Promise<UnlistenFn> {
  return listen<T>(event, (e) => handler(e.payload));
}

/** Same rule as the backend: Cyrillic-heavy text is Russian. */
export function direction(text: string): [Lang, Lang] {
  const cyr = (text.match(/[Ѐ-ӿ]/g) ?? []).length;
  const lat = (text.match(/[A-Za-z]/g) ?? []).length;
  return cyr > 0 && cyr * 3 >= lat ? ["ru", "en"] : ["en", "ru"];
}

/**
 * How a clipboard entry looks in the list: the history keeps the text exactly
 * as copied (indentation, tabs), the row shows it compactly.
 */
export function previewText(text: string): string {
  return text
    .split(/\r\n|\r|\n/)
    .map((line) => line.replace(/[ \t\u00a0]+/g, " ").trim())
    .filter(Boolean)
    .slice(0, 4)
    .join("\n");
}

/** "1,4 МБ" for the file list. */
export function sizeText(bytes: number): string {
  const units = ["Б", "КБ", "МБ", "ГБ", "ТБ"];
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit++;
  }
  const digits = unit === 0 || value >= 100 ? 0 : 1;
  return `${value.toFixed(digits).replace(".", ",")} ${units[unit]}`;
}

export function errorText(e: unknown): string {
  return typeof e === "string" ? e : e instanceof Error ? e.message : "что-то пошло не так";
}

export function timeAgo(ms: number): string {
  const s = Math.max(0, Math.round((Date.now() - ms) / 1000));
  if (s < 45) return "сейчас";
  const m = Math.round(s / 60);
  if (m < 60) return `${m} мин`;
  const h = Math.round(m / 60);
  if (h < 24) return `${h} ч`;
  return `${Math.round(h / 24)} д`;
}
