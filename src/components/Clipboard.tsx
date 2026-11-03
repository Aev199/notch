import { motion, AnimatePresence } from "motion/react";
import { api, errorText, timeAgo, type TextEntry } from "../lib/api";
import { CloseIcon } from "./Icons";

interface Props {
  entries: TextEntry[];
  onError: (message: string) => void;
}

export function Clipboard({ entries, onError }: Props) {
  if (entries.length === 0) {
    return (
      <div className="empty">
        <p>Буфер пуст</p>
        <span>Копируйте текст как обычно (Ctrl+C) — последние фрагменты соберутся здесь. Клик по строке вернёт её в буфер.</span>
      </div>
    );
  }
  return (
    <div className="clips">
      <AnimatePresence initial={false}>
        {entries.map((entry, i) => (
          <motion.div
            key={entry.id}
            layout
            className={`clip${i === 0 ? " current" : ""}`}
            initial={{ opacity: 0, y: -6 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, x: 30 }}
            transition={{ type: "spring", stiffness: 420, damping: 34 }}
            role="button"
            tabIndex={-1}
            title="Скопировать и свернуть"
            onClick={() => void api.copyEntry(entry.id, true).catch((e) => onError(errorText(e)))}
          >
            <span className="clip-text">{entry.text}</span>
            <span className="clip-meta">{timeAgo(entry.copied_at)}</span>
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
          </motion.div>
        ))}
      </AnimatePresence>
    </div>
  );
}
