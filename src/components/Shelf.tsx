import { useRef, useState } from "react";
import { motion, AnimatePresence } from "motion/react";
import { startDrag } from "@crabnebula/tauri-plugin-drag";
import { api, errorText, timeAgo, type ShelfItem } from "../lib/api";
import { CheckIcon, CopyIcon, TrashIcon } from "./Icons";

const DRAG_THRESHOLD = 5;

interface Props {
  items: ShelfItem[];
  onError: (message: string) => void;
}

export function Shelf({ items, onError }: Props) {
  const [copied, setCopied] = useState<string | null>(null);
  const press = useRef<{ id: string; x: number; y: number } | null>(null);

  if (items.length === 0) {
    return (
      <div className="empty">
        <p>Полка пуста</p>
        <span>Сделайте скриншот в буфер (Win+Shift+S или PrtScr) — он появится здесь, и его можно будет перетащить в любое окно.</span>
      </div>
    );
  }

  const drag = async (item: ShelfItem) => {
    await api.setDragging(true);
    try {
      await startDrag({ item: [item.path], icon: item.thumb_path }, () => {
        void api.setDragging(false);
      });
    } catch (e) {
      onError(errorText(e));
    } finally {
      void api.setDragging(false);
    }
  };

  const copy = async (item: ShelfItem) => {
    try {
      await api.shelfCopy(item.id);
      setCopied(item.id);
      window.setTimeout(() => setCopied((c) => (c === item.id ? null : c)), 1200);
    } catch (e) {
      onError(errorText(e));
    }
  };

  return (
    <div
      className="shelf"
      onWheel={(e) => {
        // Vertical wheel scrolls the carousel horizontally.
        if (Math.abs(e.deltaY) > Math.abs(e.deltaX)) e.currentTarget.scrollLeft += e.deltaY;
      }}
    >
      <AnimatePresence initial={false}>
        {items.map((item) => (
          <motion.figure
            key={item.id}
            className="shot"
            layout
            initial={{ opacity: 0, scale: 0.85 }}
            animate={{ opacity: 1, scale: 1 }}
            exit={{ opacity: 0, scale: 0.8 }}
            transition={{ type: "spring", stiffness: 380, damping: 30 }}
            title={`${item.width}×${item.height} — перетащите в окно или папку`}
            onPointerDown={(e) => {
              if (e.button !== 0 || (e.target as HTMLElement).closest("button")) return;
              press.current = { id: item.id, x: e.clientX, y: e.clientY };
            }}
            onPointerMove={(e) => {
              const p = press.current;
              if (!p || p.id !== item.id) return;
              if (Math.hypot(e.clientX - p.x, e.clientY - p.y) >= DRAG_THRESHOLD) {
                press.current = null;
                void drag(item);
              }
            }}
            onPointerUp={() => (press.current = null)}
            onPointerLeave={() => {
              // Leaving the card while pressed also starts the drag.
              const p = press.current;
              if (p && p.id === item.id) {
                press.current = null;
                void drag(item);
              }
            }}
          >
            <img src={item.thumb} alt="" draggable={false} />
            <figcaption>
              <span>{timeAgo(item.created_at)}</span>
              <span className="dim">
                {item.width}×{item.height}
              </span>
            </figcaption>
            <div className="shot-actions">
              <button className="icon-btn" title="Копировать" onClick={() => void copy(item)}>
                {copied === item.id ? <CheckIcon /> : <CopyIcon />}
              </button>
              <button
                className="icon-btn danger"
                title="Удалить"
                onClick={() => void api.shelfRemove(item.id).catch((e) => onError(errorText(e)))}
              >
                <TrashIcon />
              </button>
            </div>
          </motion.figure>
        ))}
      </AnimatePresence>
    </div>
  );
}
