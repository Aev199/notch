import { useMemo, useState } from "react";
import { api, errorText, timeAgo, type TextEntry } from "../lib/api";
import { CloseIcon, InsertIcon, PinIcon, SearchIcon } from "./Icons";

interface Props {
  entries: TextEntry[];
  onError: (message: string) => void;
}

/** Case- and ё-insensitive form for matching. */
const fold = (s: string) => s.toLocaleLowerCase("ru").replace(/ё/g, "е");

export function Clipboard({ entries, onError }: Props) {
  const [query, setQuery] = useState("");

  const shown = useMemo(() => {
    const q = fold(query.trim());
    const matched = q ? entries.filter((e) => fold(e.text).includes(q)) : entries;
    // Pinned first, the rest newest first (backend order).
    return [...matched.filter((e) => e.pinned), ...matched.filter((e) => !e.pinned)];
  }, [entries, query]);

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
      <label className="search">
        <SearchIcon width={14} height={14} />
        <input
          value={query}
          placeholder="Поиск"
          spellCheck={false}
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && shown[0]) {
              e.preventDefault();
              paste(shown[0].id);
            } else if (e.key === "Escape" && query) {
              // First Esc clears the search, the second one closes the island.
              e.stopPropagation();
              setQuery("");
            }
          }}
        />
        {query && <span className="dim">{shown.length ? `${shown.length} · Enter — вставить` : "ничего"}</span>}
      </label>
      <div className="clips">
        {shown.map((entry) => (
          <div
            key={entry.id}
            className={`clip${entry.pinned ? " pinned" : ""}`}
            role="button"
            tabIndex={-1}
            title="Клик — скопировать и свернуть"
            onClick={() => void api.copyEntry(entry.id, true).catch((e) => onError(errorText(e)))}
          >
            {entry.pinned && <PinIcon className="pin-mark" width={12} height={12} />}
            <span className="clip-text">{entry.text}</span>
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
    </div>
  );
}
