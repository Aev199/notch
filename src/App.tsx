import { useEffect, useRef, useState } from "react";
import { AnimatePresence, motion, MotionConfig } from "motion/react";
import { api, on, type IslandEvent, type Settings, type ShelfItem, type Snapshot, type Tab, type TextEntry } from "./lib/api";
import { Shelf } from "./components/Shelf";
import { Clipboard } from "./components/Clipboard";
import { Translate } from "./components/Translate";
import { SettingsPanel } from "./components/SettingsPanel";
import { ClipIcon, CloseIcon, GearIcon, ImageIcon, TranslateIcon } from "./components/Icons";

/** Logical sizes; the collapsed one must match the backend hitbox. */
const COMPACT = { width: 180, height: 32 };
const EXPANDED = { width: 520, height: 252 };

// Spring tuned after PILLAR's "snappy" preset: quick, with a hint of overshoot.
const SPRING = { type: "spring" as const, stiffness: 420, damping: 32, mass: 0.9 };

const TABS: { id: Exclude<Tab, "settings">; title: string; Icon: typeof ImageIcon }[] = [
  { id: "shelf", title: "Полка", Icon: ImageIcon },
  { id: "clipboard", title: "Буфер", Icon: ClipIcon },
  { id: "translate", title: "Перевод", Icon: TranslateIcon },
];

const PROVIDER_NAMES = { google: "Google", deepl: "DeepL", llm: "Локальная модель" } as const;

export default function App() {
  const [expanded, setExpanded] = useState(false);
  const [tab, setTab] = useState<Tab>("shelf");
  const [texts, setTexts] = useState<TextEntry[]>([]);
  const [shelf, setShelf] = useState<ShelfItem[]>([]);
  const [settings, setSettings] = useState<Settings | null>(null);
  const [snapshot, setSnapshot] = useState<Snapshot | null>(null);
  const [autostart, setAutostart] = useState(false);
  const [incoming, setIncoming] = useState<{ text: string; seq: number } | null>(null);
  const [flash, setFlash] = useState<"shelf" | "clipboard" | null>(null);
  const [toast, setToast] = useState<string | null>(null);
  const flashTimer = useRef<number>();
  const toastTimer = useRef<number>();

  useEffect(() => {
    void api.setIslandSize(EXPANDED.width, EXPANDED.height);
    void api.snapshot().then((s) => {
      setSnapshot(s);
      setSettings(s.settings);
      setTexts(s.texts);
      setShelf(s.shelf);
      setAutostart(s.autostart);
      setExpanded(s.expanded);
    });
    let seq = 0;
    const subs = [
      on<IslandEvent>("island", (e) => {
        setExpanded(e.expanded);
        if (e.tab) setTab(e.tab);
        if (!e.expanded) (document.activeElement as HTMLElement | null)?.blur();
      }),
      on<TextEntry[]>("texts", setTexts),
      on<ShelfItem[]>("shelf", setShelf),
      on<string>("translate-request", (text) => setIncoming({ text, seq: ++seq })),
      on<"shelf" | "clipboard">("captured", (kind) => {
        setFlash(kind);
        window.clearTimeout(flashTimer.current);
        flashTimer.current = window.setTimeout(() => setFlash(null), 1400);
      }),
      on<boolean>("input-mode", (active) => {
        if (!active) (document.activeElement as HTMLElement | null)?.blur();
      }),
    ];
    return () => subs.forEach((p) => void p.then((off) => off()));
  }, []);

  const showError = (message: string) => {
    setToast(message);
    window.clearTimeout(toastTimer.current);
    toastTimer.current = window.setTimeout(() => setToast(null), 3500);
  };

  const size = expanded ? EXPANDED : COMPACT;
  const providerName = settings ? PROVIDER_NAMES[settings.provider] : "";

  return (
    <MotionConfig reducedMotion="user">
      <div className="stage">
        <motion.div
          className={`island${expanded ? " open" : ""}${flash ? " flash" : ""}`}
          initial={false}
          animate={{
            width: size.width,
            height: size.height,
            borderBottomLeftRadius: expanded ? 26 : 16,
            borderBottomRightRadius: expanded ? 26 : 16,
          }}
          transition={SPRING}
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
          <AnimatePresence initial={false} mode="popLayout">
            {expanded ? (
              <motion.div
                key="full"
                className="full"
                initial={{ opacity: 0, filter: "blur(6px)" }}
                animate={{ opacity: 1, filter: "blur(0px)", transition: { delay: 0.06, duration: 0.2 } }}
                exit={{ opacity: 0, filter: "blur(6px)", transition: { duration: 0.1 } }}
              >
                <header className="tabs">
                  {TABS.map(({ id, title, Icon }) => (
                    <button key={id} className={`tab${tab === id ? " active" : ""}`} onClick={() => setTab(id)}>
                      {tab === id && <motion.span layoutId="tab-pill" className="tab-pill" transition={SPRING} />}
                      <Icon width={15} height={15} />
                      <span>{title}</span>
                      {id === "shelf" && shelf.length > 0 && <em>{shelf.length}</em>}
                      {id === "clipboard" && texts.length > 0 && <em>{texts.length}</em>}
                    </button>
                  ))}
                  <span className="push" />
                  {tab === "shelf" && shelf.length > 0 && (
                    <button className="btn ghost" onClick={() => void api.shelfClear()}>
                      Очистить
                    </button>
                  )}
                  {tab === "clipboard" && texts.length > 0 && (
                    <button className="btn ghost" onClick={() => void api.clearEntries()}>
                      Очистить
                    </button>
                  )}
                  <button
                    className={`icon-btn${tab === "settings" ? " active" : ""}`}
                    title="Настройки"
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
                  {tab === "translate" && <Translate incoming={incoming} providerName={providerName} />}
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
                <AnimatePresence>
                  {toast && (
                    <motion.div
                      className="toast"
                      initial={{ opacity: 0, y: 8 }}
                      animate={{ opacity: 1, y: 0 }}
                      exit={{ opacity: 0, y: 8 }}
                    >
                      {toast}
                    </motion.div>
                  )}
                </AnimatePresence>
              </motion.div>
            ) : (
              <motion.div
                key="compact"
                className="compact"
                initial={{ opacity: 0 }}
                animate={{ opacity: 1, transition: { delay: 0.08 } }}
                exit={{ opacity: 0, transition: { duration: 0.08 } }}
              >
                <span className={`count${flash === "shelf" ? " hot" : ""}`}>
                  <ImageIcon width={13} height={13} />
                  {shelf.length}
                </span>
                <span className="cam" />
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
