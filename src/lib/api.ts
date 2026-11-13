import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type Tab = "shelf" | "clipboard" | "translate" | "settings";
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
  compact: [number, number];
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
  /** Put the caret into the translator field. */
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
