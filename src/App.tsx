import { useEffect, useRef, useState } from "react";
import { AnimatePresence, motion, MotionConfig } from "motion/react";
import {
  api,
  on,
  type IslandEvent,
  type Mode,
  type Settings,
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
import { Translate, type Incoming } from "./components/Translate";
import { SettingsPanel } from "./components/SettingsPanel";
import { ClipIcon, CloseIcon, GearIcon, ImageIcon } from "./components/Icons";
import { updatePending } from "./components/Updates";

/** Logical sizes; compact and open must match the backend hitboxes. */
const SIZES: Record<Mode, { width: number; height: number; radius: number }> = {
  thin: { width: 140, height: 4, radius: 2 },
  compact: { width: 180, height: 32, radius: 12 },
  open: { width: 520, height: 252, radius: 16 },
};

/** Plain ease-out, no overshoot. */
const RESIZE = { type: "tween" as const, ease: [0.2, 0.8, 0.2, 1] as const, duration: 0.18 };

const TABS: { id: Exclude<Tab, "settings">; title: string }[] = [
  { id: "shelf", title: "Полка" },
  { id: "clipboard", title: "Буфер" },
  { id: "translate", title: "Перевод" },
];

const PROVIDER_NAMES = { offline: "офлайн", google: "Google", deepl: "DeepL", llm: "локальная модель" } as const;

export default function App() {
  const [mode, setMode] = useState<Mode>("compact");
  const [tab, setTab] = useState<Tab>("shelf");
  const [texts, setTexts] = useState<TextEntry[]>([]);
  const [shelf, setShelf] = useState<ShelfItem[]>([]);
  const [settings, setSettings] = useState<Settings | null>(null);
  const [snapshot, setSnapshot] = useState<Snapshot | null>(null);
  const [autostart, setAutostart] = useState(false);
  const [incoming, setIncoming] = useState<Incoming | null>(null);
  const [flash, setFlash] = useState<"shelf" | "clipboard" | null>(null);
  const [toast, setToast] = useState<string | null>(null);
  const [update, setUpdate] = useState<UpdateStatus | null>(null);
  const flashTimer = useRef<number>();
  const toastTimer = useRef<number>();
  // One counter for every request to the translator, whatever its source.
  const requestSeq = useRef(0);
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
        if (e.mode !== "open") (document.activeElement as HTMLElement | null)?.blur();
      }),
      on<TextEntry[]>("texts", setTexts),
      on<ShelfItem[]>("shelf", setShelf),
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

  const showError = (message: string) => {
    setToast(message);
    window.clearTimeout(toastTimer.current);
    toastTimer.current = window.setTimeout(() => setToast(null), 3500);
  };

  const size = SIZES[mode];
  const providerName = settings ? PROVIDER_NAMES[settings.provider] : "";
  const counts: Record<string, number> = { shelf: shelf.length, clipboard: texts.length };

  return (
    <MotionConfig reducedMotion="user">
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
                  {TABS.map(({ id, title }) => (
                    <button key={id} className={`tab${tab === id ? " active" : ""}`} onClick={() => setTab(id)}>
                      {title}
                      {counts[id] > 0 && <span className="tab-count">{counts[id]}</span>}
                    </button>
                  ))}
                  <span className="push" />
                  {tab === "shelf" && shelf.some((i) => !i.group) && (
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
                  {tab === "shelf" && <Shelf items={shelf} onError={showError} />}
                  {tab === "clipboard" && <Clipboard entries={texts} onError={showError} />}
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
                </main>
                {toast && <div className="toast">{toast}</div>}
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
                <span className={`count${flash === "clipboard" ? " hot" : ""}`}>
                  <ClipIcon width={13} height={13} />
                  {texts.length}
                </span>
              </motion.div>
            )}
          </AnimatePresence>
        </motion.div>
      </div>
    </MotionConfig>
  );
}
