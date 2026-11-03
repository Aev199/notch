import { useEffect, useRef, useState } from "react";
import { motion, AnimatePresence } from "motion/react";
import { api, direction, errorText, type Translation } from "../lib/api";
import { CheckIcon, CopyIcon, PasteIcon } from "./Icons";

interface Props {
  /** Text pushed by the backend (double Ctrl+C, hotkey); `seq` forces a rerun. */
  incoming: { text: string; seq: number } | null;
  providerName: string;
}

const AUTO_DELAY_MS = 700;

export function Translate({ incoming, providerName }: Props) {
  const [source, setSource] = useState("");
  const [result, setResult] = useState<Translation | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [copied, setCopied] = useState(false);
  const request = useRef(0);
  const timer = useRef<number>();

  const run = async (text: string) => {
    window.clearTimeout(timer.current);
    const trimmed = text.trim();
    if (!trimmed) {
      setResult(null);
      setError(null);
      return;
    }
    const id = ++request.current;
    setBusy(true);
    setError(null);
    try {
      const t = await api.translate(trimmed);
      if (id === request.current) setResult(t);
    } catch (e) {
      if (id === request.current) {
        setResult(null);
        setError(errorText(e));
      }
    } finally {
      if (id === request.current) setBusy(false);
    }
  };

  useEffect(() => {
    if (!incoming) return;
    setSource(incoming.text);
    void run(incoming.text);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [incoming?.seq]);

  useEffect(() => () => window.clearTimeout(timer.current), []);

  const onType = (value: string) => {
    setSource(value);
    window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => void run(value), AUTO_DELAY_MS);
  };

  const paste = async () => {
    const text = await api.clipboardText();
    if (text) {
      setSource(text);
      void run(text);
    }
  };

  const copy = async () => {
    if (!result) return;
    await api.copyText(result.text);
    setCopied(true);
    window.setTimeout(() => setCopied(false), 1200);
  };

  const [from, to] = result ? [result.from, result.to] : direction(source);

  return (
    <div className="translate">
      <div className="pane">
        <div className="pane-head">
          <span className="lang">
            {from.toUpperCase()} <span className="arrow">→</span> {to.toUpperCase()}
          </span>
          <span className="dim">{providerName}</span>
          <button className="icon-btn small push" title="Вставить из буфера" onClick={() => void paste()}>
            <PasteIcon width={14} height={14} />
          </button>
        </div>
        <textarea
          value={source}
          placeholder="Введите текст или нажмите Ctrl+C дважды в любой программе"
          spellCheck={false}
          onChange={(e) => onType(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
              e.preventDefault();
              void run(source);
            }
          }}
        />
      </div>
      <div className={`pane result${busy ? " busy" : ""}`}>
        <div className="pane-head">
          <span className="lang">{to.toUpperCase()}</span>
          {busy && <span className="spinner" aria-label="перевожу" />}
          <button
            className="icon-btn small push"
            title="Скопировать перевод"
            disabled={!result}
            onClick={() => void copy()}
          >
            {copied ? <CheckIcon width={14} height={14} /> : <CopyIcon width={14} height={14} />}
          </button>
        </div>
        <AnimatePresence mode="wait">
          <motion.div
            key={error ? "e" : result?.text ?? "none"}
            className={`result-text${error ? " error" : ""}${result ? " clickable" : ""}`}
            initial={{ opacity: 0, y: 4 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0 }}
            transition={{ duration: 0.15 }}
            title={result ? "Клик — скопировать" : undefined}
            onClick={() => void copy()}
          >
            {error ?? result?.text ?? <span className="dim">Здесь появится перевод</span>}
          </motion.div>
        </AnimatePresence>
      </div>
    </div>
  );
}
