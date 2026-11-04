import { api, errorText, timeAgo, type TextEntry } from "../lib/api";
import { CloseIcon, InsertIcon } from "./Icons";

interface Props {
  entries: TextEntry[];
  onError: (message: string) => void;
}

export function Clipboard({ entries, onError }: Props) {
  if (entries.length === 0) {
    return (
      <div className="empty">
        <p>Здесь будет скопированный текст</p>
        <span>Клик по строке — скопировать, «Вставить» — сразу в поле, где стоит курсор.</span>
      </div>
    );
  }
  return (
    <div className="clips">
      {entries.map((entry) => (
        <div
          key={entry.id}
          className="clip"
          role="button"
          tabIndex={-1}
          title="Клик — скопировать и свернуть"
          onClick={() => void api.copyEntry(entry.id, true).catch((e) => onError(errorText(e)))}
        >
          <span className="clip-text">{entry.text}</span>
          <span className="clip-meta">{timeAgo(entry.copied_at)}</span>
          <span className="clip-actions">
            <button
              className="btn"
              title="Вставить туда, где стоит курсор"
              onClick={(e) => {
                e.stopPropagation();
                void api.pasteEntry(entry.id).catch((err) => onError(errorText(err)));
              }}
            >
              <InsertIcon width={13} height={13} />
              Вставить
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
  );
}
