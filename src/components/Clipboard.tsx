import { useEffect, useMemo, useRef, useState } from "react";
import { api, errorText, previewText, timeAgo, type TextEntry } from "../lib/api";
import { CloseIcon, InsertIcon, PinIcon, ProjectIcon, SearchIcon } from "./Icons";
import { SendToProject } from "./SendToProject";

interface Props {
  entries: TextEntry[];
  /** Changes when the history hotkey asks for the keyboard: caret into the search. */
  focusSeq: number;
  onError: (message: string) => void;
  /** A short message that something was done. */
  onNotice: (message: string) => void;
}

/** Case- and ё-insensitive form for matching. */
const fold = (s: string) => s.toLocaleLowerCase("ru").replace(/ё/g, "е");

// Survives switching tabs (the component unmounts) within one session.
const memory = { handledFocus: 0 };

export function Clipboard({ entries, focusSeq, onError, onNotice }: Props) {
  const [query, setQuery] = useState("");
  // Row picked with the arrow keys; Enter pastes it.
  const [selected, setSelected] = useState(0);
  const search = useRef<HTMLInputElement>(null);
  const list = useRef<HTMLDivElement>(null);

  // Opened by the hotkey: a fresh search with the caret in it.
  useEffect(() => {
    if (focusSeq === memory.handledFocus) return;
    memory.handledFocus = focusSeq;
    setQuery("");
    setSelected(0);
    search.current?.focus();
  }, [focusSeq]);

  useEffect(() => {
    list.current?.querySelector(".clip.selected")?.scrollIntoView({ block: "nearest" });
  }, [selected]);

  const shown = useMemo(() => {
    const q = fold(query.trim());
    const matched = q ? entries.filter((e) => fold(e.text).includes(q)) : entries;
    // Pinned first, the rest newest first (backend order).
    return [...matched.filter((e) => e.pinned), ...matched.filter((e) => !e.pinned)];
  }, [entries, query]);

  // The arrow-key row is marked only while the keyboard is in the search field.
  const [keyboard, setKeyboard] = useState(false);
  // The text being sent to a Papka project.
  const [sending, setSending] = useState<TextEntry | null>(null);

  const paste = (id: number) => void api.pasteEntry(id).catch((err) => onError(errorText(err)));

  if (entries.length === 0) {
    return (
      <div className="empty">
        <p>Здесь будет скопированный текст</p>
        <span>Клик по строке — скопировать, «Вставить» — сразу в поле, где стоит курсор.</span>
      </div>
    );
  }

  return (
    <div className="clip-wrap">
      <label className="search" onFocus={() => setKeyboard(true)} onBlur={() => setKeyboard(false)}>
        <SearchIcon width={14} height={14} />
        <input
          ref={search}
          value={query}
          placeholder="Поиск"
          spellCheck={false}
          onChange={(e) => {
            setQuery(e.target.value);
            setSelected(0);
          }}
          onKeyDown={(e) => {
            const row = shown[Math.min(selected, shown.length - 1)];
            if (e.key === "Enter" && row) {
              e.preventDefault();
              paste(row.id);
            } else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
              e.preventDefault();
              const step = e.key === "ArrowDown" ? 1 : -1;
              setSelected((i) => Math.max(0, Math.min(shown.length - 1, Math.min(i, shown.length - 1) + step)));
            } else if (e.key === "Escape" && query) {
              // First Esc clears the search, the second one closes the island.
              e.stopPropagation();
              setQuery("");
            }
          }}
        />
        {(query || keyboard) && <span className="dim">{shown.length ? `${query ? `${shown.length} · ` : ""}↑↓ · Enter — вставить` : "ничего"}</span>}
      </label>
      <div className="clips" ref={list}>
        {shown.map((entry, index) => (
          <div
            key={entry.id}
            className={`clip${entry.pinned ? " pinned" : ""}${keyboard && index === Math.min(selected, shown.length - 1) ? " selected" : ""}`}
            role="button"
            tabIndex={-1}
            title="Клик — скопировать и свернуть"
            onClick={() => void api.copyEntry(entry.id, true).catch((e) => onError(errorText(e)))}
          >
            {entry.pinned && <PinIcon className="pin-mark" width={12} height={12} />}
            <span className="clip-text">{previewText(entry.text)}</span>
            <span className="clip-meta">{timeAgo(entry.copied_at)}</span>
            <span className="clip-actions">
              <button
                className="btn"
                title="Вставить туда, где стоит курсор"
                onClick={(e) => {
                  e.stopPropagation();
                  paste(entry.id);
                }}
              >
                <InsertIcon width={13} height={13} />
                Вставить
              </button>
              <button
                className="icon-btn small"
                title="В проект Papka…"
                onClick={(e) => {
                  e.stopPropagation();
                  setSending(entry);
                }}
              >
                <ProjectIcon width={13} height={13} />
              </button>
              <button
                className={`icon-btn small${entry.pinned ? " active" : ""}`}
                title={entry.pinned ? "Открепить" : "Закрепить: не удаляется и хранится между запусками"}
                onClick={(e) => {
                  e.stopPropagation();
                  void api.pinEntry(entry.id, !entry.pinned);
                }}
              >
                <PinIcon width={13} height={13} />
              </button>
              <button
                className="icon-btn small"
                title="Удалить из истории"
                onClick={(e) => {
                  e.stopPropagation();
                  void api.removeEntry(entry.id);
                }}
              >
                <CloseIcon width={13} height={13} />
              </button>
            </span>
          </div>
        ))}
      </div>
      {sending && (
        <SendToProject
          what="текст"
          send={(project, caption) => api.papkaSendText(sending.text, project, caption)}
          onDone={(message) => {
            setSending(null);
            onNotice(message);
          }}
          onClose={() => setSending(null)}
        />
      )}
    </div>
  );
}
