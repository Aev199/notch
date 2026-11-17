import { useEffect, useRef, useState } from "react";
import { AnimatePresence, motion } from "motion/react";
import {
  api,
  errorText,
  on,
  type IslandEvent,
  type Mode,
  type Settings,
  type FileEntry,
  type ShelfItem,
  type Snapshot,
  type Tab,
  type TextEntry,
  type TranslateRequest,
  type UpdateStatus,
} from "./lib/api";
import { translateOffline } from "./lib/offline";
import { useMinuteTick } from "./lib/time";
import { Shelf } from "./components/Shelf";
import { Clipboard } from "./components/Clipboard";
import { Files } from "./components/Files";
import { Translate, type Incoming } from "./components/Translate";
import { SettingsPanel } from "./components/SettingsPanel";
import { AddImageIcon, ClipIcon, CloseIcon, FileIcon, GearIcon, ImageIcon } from "./components/Icons";
import { updatePending } from "./components/Updates";

/** Logical sizes; compact and open must match the backend hitboxes. */
const SIZES: Record<Mode, { width: number; height: number; radius: number }> = {
  thin: { width: 140, height: 4, radius: 2 },
  compact: { width: 180, height: 32, radius: 12 },
  open: { width: 520, height: 252, radius: 16 },
};

/** Plain ease-out, no overshoot. */
const EASE = [0.2, 0.8, 0.2, 1] as const;
const RESIZE = { type: "tween" as const, ease: EASE, duration: 0.18 };
const TAB_LINE = { type: "tween" as const, ease: EASE, duration: 0.16 };

/** Left to right as on screen; the gear is last. */
const TAB_ORDER: Tab[] = ["shelf", "files", "clipboard", "translate", "settings"];

const TABS: { id: Exclude<Tab, "settings">; title: string }[] = [
  { id: "shelf", title: "Полка" },
  { id: "files", title: "Файлы" },
  { id: "clipboard", title: "Буфер" },
  { id: "translate", title: "Перевод" },
];

const PROVIDER_NAMES = { offline: "офлайн", google: "Google", deepl: "DeepL", llm: "локальная модель" } as const;

export default function App() {
  const [mode, setMode] = useState<Mode>("compact");
  const [tab, setTab] = useState<Tab>("shelf");
  const [texts, setTexts] = useState<TextEntry[]>([]);
  const [shelf, setShelf] = useState<ShelfItem[]>([]);
  const [files, setFiles] = useState<FileEntry[]>([]);
  // Bumped when the history hotkey puts the caret into the clipboard search.
  const [clipFocus, setClipFocus] = useState(0);
  const [settings, setSettings] = useState<Settings | null>(null);
  const [snapshot, setSnapshot] = useState<Snapshot | null>(null);
  const [autostart, setAutostart] = useState(false);
  const [incoming, setIncoming] = useState<Incoming | null>(null);
  const [flash, setFlash] = useState<"shelf" | "clipboard" | null>(null);
  const [toast, setToast] = useState<{ text: string; ok: boolean } | null>(null);
  const [update, setUpdate] = useState<UpdateStatus | null>(null);
  const flashTimer = useRef<number>();
  const toastTimer = useRef<number>();
  // One counter for every request to the translator, whatever its source.
  const requestSeq = useRef(0);
  // Which way the content slides when the tab changes.
  const lastTab = useRef<Tab>(tab);
  // Re-render once a minute so "5 мин" labels stay true while open.
  useMinuteTick(mode === "open");

  useEffect(() => {
    void api.setIslandSize(SIZES.open.width, SIZES.open.height);
    let warm: number | undefined;
    void api.updateInfo().then((i) => setUpdate(i.status)).catch(() => {});
    void api.snapshot().then((s) => {
      setSnapshot(s);
      setSettings(s.settings);
      setTexts(s.texts);
      setShelf(s.shelf);
      setFiles(s.files);
      setAutostart(s.autostart);
      setMode(s.mode);
      if (s.settings.provider === "offline") {
        // Load the engine and both models in the background so the first
        // double Ctrl+C is answered right away.
        warm = window.setTimeout(() => {
          translateOffline("привет", "ru", "en")
            .then(() => translateOffline("hello", "en", "ru"))
            .catch(() => {});
        }, 1500);
      }
    });
    const subs = [
      on<IslandEvent>("island", (e) => {
        setMode(e.mode);
        if (e.tab) setTab(e.tab);
        if (e.focus && e.tab === "clipboard") setClipFocus((n) => n + 1);
        if (e.mode !== "open") (document.activeElement as HTMLElement | null)?.blur();
      }),
      on<TextEntry[]>("texts", setTexts),
      on<ShelfItem[]>("shelf", setShelf),
      on<FileEntry[]>("files", setFiles),
      on<TranslateRequest>("translate-request", (r) => setIncoming({ ...r, seq: ++requestSeq.current })),
      on<"shelf" | "clipboard">("captured", (kind) => {
        setFlash(kind);
        window.clearTimeout(flashTimer.current);
        flashTimer.current = window.setTimeout(() => setFlash(null), 1400);
      }),
      on<UpdateStatus>("update", setUpdate),
      // After sleep the backend rebuilds the window; nudge the page to repaint too.
      on<null>("repaint", () => {
        const root = document.documentElement;
        root.style.opacity = "0.99";
        requestAnimationFrame(() => requestAnimationFrame(() => (root.style.opacity = "")));
      }),
      on<boolean>("input-mode", (active) => {
        if (!active) (document.activeElement as HTMLElement | null)?.blur();
      }),
    ];
    return () => {
      window.clearTimeout(warm);
      subs.forEach((p) => void p.then((off) => off()));
    };
  }, []);

  const showToast = (text: string, ok: boolean) => {
    setToast({ text, ok });
    window.clearTimeout(toastTimer.current);
    toastTimer.current = window.setTimeout(() => setToast(null), 3500);
  };
  const showError = (message: string) => showToast(message, false);
  /** Something done, not wrong: «Отправлено в …». */
  const showNotice = (message: string) => showToast(message, true);

  const size = SIZES[mode];
  const providerName = settings ? PROVIDER_NAMES[settings.provider] : "";
  const counts: Record<string, number> = { shelf: shelf.length, files: files.length, clipboard: texts.length };
  const filesOn = settings?.filesShelf ?? false;
  const tabs = TABS.filter((t) => t.id !== "files" || filesOn);
  // The tab was switched off in the settings while it was open.
  const shownTab: Tab = tab === "files" && !filesOn ? "shelf" : tab;
  const slide = Math.sign(TAB_ORDER.indexOf(shownTab) - TAB_ORDER.indexOf(lastTab.current));
  useEffect(() => {
    lastTab.current = shownTab;
  }, [shownTab]);

  return (
    <>
      <div className="stage">
        <motion.div
          className={`island is-${mode}`}
          initial={false}
          animate={{
            width: size.width,
            height: size.height,
            borderBottomLeftRadius: size.radius,
            borderBottomRightRadius: size.radius,
          }}
          transition={RESIZE}
          onFocusCapture={(e) => {
            const el = e.target as HTMLElement;
            if (el.matches("input, textarea, select")) void api.beginInput();
          }}
          onKeyDown={(e) => {
            if (e.key === "Escape") {
              void api.endInput();
              void api.close();
            }
          }}
        >
          <AnimatePresence initial={false}>
            {mode === "open" && (
              <motion.div
                key="full"
                className="full"
                initial={{ opacity: 0 }}
                animate={{ opacity: 1, transition: { duration: 0.12, delay: 0.04 } }}
                exit={{ opacity: 0, transition: { duration: 0.05 } }}
              >
                <header className="tabs">
                  {tabs.map(({ id, title }) => (
                    <button key={id} className={`tab${shownTab === id ? " active" : ""}`} onClick={() => setTab(id)}>
                      {title}
                      {counts[id] > 0 && <span className="tab-count">{counts[id]}</span>}
                      {shownTab === id && <motion.span layoutId="tab-line" className="tab-line" transition={TAB_LINE} />}
                    </button>
                  ))}
                  <span className="push" />
                  {shownTab === "files" && files.length > 0 && (
                    <button className="btn" title="Убрать всё с полки (сами файлы останутся на местах)" onClick={() => void api.filesClear()}>
                      Очистить
                    </button>
                  )}
                  {shownTab === "shelf" && (
                    <button
                      className="icon-btn"
                      title="Положить на полку картинку из буфера обмена (если она не попала сама)"
                      onClick={() => void api.shelfFromClipboard().catch((e) => showError(errorText(e)))}
                    >
                      <AddImageIcon />
                    </button>
                  )}
                  {shownTab === "shelf" && shelf.some((i) => !i.group) && (
                    <button className="btn" title="Удалить скриншоты вне групп" onClick={() => void api.shelfClear()}>
                      Очистить
                    </button>
                  )}
                  {tab === "clipboard" && texts.length > 0 && (
                    <button className="btn" onClick={() => void api.clearEntries()}>
                      Очистить
                    </button>
                  )}
                  <button
                    className={`icon-btn${tab === "settings" ? " active" : ""}${updatePending(update) ? " badge-dot" : ""}`}
                    title={updatePending(update) ? "Настройки — есть обновление" : "Настройки"}
                    onClick={() => setTab(tab === "settings" ? "shelf" : "settings")}
                  >
                    <GearIcon />
                  </button>
                  <button className="icon-btn" title="Свернуть (Esc)" onClick={() => void api.close()}>
                    <CloseIcon />
                  </button>
                </header>
                <main className="content">
                  <motion.div
                    key={shownTab}
                    className="tab-pane"
                    initial={slide ? { opacity: 0, x: slide * 10 } : false}
                    animate={{ opacity: 1, x: 0 }}
                    transition={{ type: "tween", ease: EASE, duration: 0.16 }}
                  >
                  {shownTab === "shelf" && <Shelf items={shelf} onError={showError} onNotice={showNotice} />}
                  {shownTab === "files" && snapshot && <Files entries={files} icon={snapshot.fileIcon} onError={showError} onNotice={showNotice} />}
                  {tab === "clipboard" && <Clipboard entries={texts} focusSeq={clipFocus} onError={showError} onNotice={showNotice} />}
                  {tab === "translate" && settings && (
                    <Translate incoming={incoming} provider={settings.provider} providerName={providerName} />
                  )}
                  {tab === "settings" && settings && snapshot && (
                    <SettingsPanel
                      settings={settings}
                      autostart={autostart}
                      dataDir={snapshot.dataDir}
                      onSaved={setSettings}
                      onAutostart={setAutostart}
                    />
                  )}
                  </motion.div>
                </main>
                {toast && <div className={`toast${toast.ok ? " ok" : ""}`}>{toast.text}</div>}
              </motion.div>
            )}
            {mode === "compact" && (
              <motion.div
                key="compact"
                className="compact"
                initial={{ opacity: 0 }}
                animate={{ opacity: 1, transition: { duration: 0.1, delay: 0.05 } }}
                exit={{ opacity: 0, transition: { duration: 0.05 } }}
              >
                <span className={`count${flash === "shelf" ? " hot" : ""}`}>
                  <ImageIcon width={13} height={13} />
                  {shelf.length}
                </span>
                {filesOn && files.length > 0 && (
                  <span className="count">
                    <FileIcon width={13} height={13} />
                    {files.length}
                  </span>
                )}
                <span className={`count${flash === "clipboard" ? " hot" : ""}`}>
                  <ClipIcon width={13} height={13} />
                  {texts.length}
                </span>
              </motion.div>
            )}
          </AnimatePresence>
        </motion.div>
      </div>
    </>
  );
}
