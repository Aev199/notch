import { useEffect, useRef, useState } from "react";
import { startDrag } from "@crabnebula/tauri-plugin-drag";
import { api, errorText, timeAgo, type ShelfItem } from "../lib/api";
import { BackIcon, CheckIcon, CopyIcon, InsertIcon, TextIcon, TrashIcon } from "./Icons";

interface Props {
  item: ShelfItem;
  onBack: () => void;
  onTranslate: (text: string) => void;
  onError: (message: string) => void;
}

type Ocr = { state: "idle" } | { state: "busy" } | { state: "done"; text: string } | { state: "error"; message: string };

// Recognised text per screenshot, kept for the session (OCR takes a moment).
const ocrCache = new Map<string, string>();

export function Preview({ item, onBack, onTranslate, onError }: Props) {
  const [url, setUrl] = useState<string | null>(null);
  const [ocr, setOcr] = useState<Ocr>(() => {
    const text = ocrCache.get(item.id);
    return text !== undefined ? { state: "done", text } : { state: "idle" };
  });
  const [copied, setCopied] = useState<"image" | "text" | null>(null);
  const press = useRef<{ x: number; y: number } | null>(null);

  useEffect(() => {
    let objectUrl: string | null = null;
    let alive = true;
    api
      .shelfImage(item.id)
      .then((bytes) => {
        if (!alive) return;
        objectUrl = URL.createObjectURL(new Blob([bytes], { type: "image/png" }));
        setUrl(objectUrl);
      })
      .catch(() => setUrl(item.thumb));
    return () => {
      alive = false;
      if (objectUrl) URL.revokeObjectURL(objectUrl);
    };
  }, [item.id, item.thumb]);

  const recognize = async (): Promise<string | null> => {
    if (ocr.state === "done") return ocr.text;
    setOcr({ state: "busy" });
    try {
      const { text } = await api.shelfOcr(item.id);
      ocrCache.set(item.id, text);
      setOcr({ state: "done", text });
      return text;
    } catch (e) {
      setOcr({ state: "error", message: errorText(e) });
      return null;
    }
  };

  const flash = (what: "image" | "text") => {
    setCopied(what);
    window.setTimeout(() => setCopied(null), 1200);
  };

  const dragOut = async () => {
    await api.setDragging(true);
    try {
      await startDrag({ item: [item.path], icon: item.thumb_path }, () => void api.setDragging(false));
    } catch (e) {
      onError(errorText(e));
    } finally {
      void api.setDragging(false);
    }
  };

  const text = ocr.state === "done" ? ocr.text : null;

  return (
    <div className="preview">
      <div className="group-bar">
        <button className="btn back" onClick={onBack} title="Назад к полке (Esc)">
          <BackIcon width={14} height={14} />
          Полка
        </button>
        <span className="dim">
          {item.width}×{item.height} · {timeAgo(item.created_at)}
        </span>
        <span className="push" />
        <button className="btn" disabled={ocr.state === "busy"} onClick={() => void recognize()} title="Распознать текст (офлайн, Windows OCR)">
          <TextIcon width={13} height={13} />
          Текст
        </button>
        <button
          className="btn"
          disabled={ocr.state === "busy"}
          onClick={async () => {
            const t = await recognize();
            if (t) onTranslate(t);
          }}
          title="Распознать и перевести"
        >
          Перевести
        </button>
        <button
          className="icon-btn small"
          title="Копировать картинку"
          onClick={() => void api.shelfCopy(item.id).then(() => flash("image")).catch((e) => onError(errorText(e)))}
        >
          {copied === "image" ? <CheckIcon width={13} height={13} /> : <CopyIcon width={13} height={13} />}
        </button>
        <button className="btn" title="Открыть в программе просмотра" onClick={() => void api.shelfOpen(item.id)}>
          Открыть
        </button>
        <button
          className="icon-btn small danger"
          title="Удалить"
          onClick={() => {
            void api.shelfRemove(item.id);
            onBack();
          }}
        >
          <TrashIcon width={13} height={13} />
        </button>
      </div>
      <div className={`preview-body${ocr.state === "idle" ? "" : " split"}`}>
        <div
          className="preview-image"
          title="Перетащите за пределы чёлки, чтобы вставить файл в другое окно"
          onPointerDown={(e) => e.button === 0 && (press.current = { x: e.clientX, y: e.clientY })}
          onPointerMove={(e) => {
            const p = press.current;
            if (p && Math.hypot(e.clientX - p.x, e.clientY - p.y) > 5) {
              press.current = null;
              void dragOut();
            }
          }}
          onPointerUp={() => (press.current = null)}
        >
          {url && <img src={url} alt="" draggable={false} />}
        </div>
        {ocr.state !== "idle" && (
          <div className="preview-text">
            {ocr.state === "busy" && <span className="dim">Распознаю…</span>}
            {ocr.state === "error" && <span className="error">{ocr.message}</span>}
            {text !== null && (
              <>
                <div className="ocr-text">{text}</div>
                <div className="ocr-actions">
                  <button
                    className="btn"
                    onClick={() => void api.copyText(text).then(() => flash("text")).catch((e) => onError(errorText(e)))}
                  >
                    {copied === "text" ? <CheckIcon width={13} height={13} /> : <CopyIcon width={13} height={13} />}
                    Копировать
                  </button>
                  <button className="btn" onClick={() => void api.pasteText(text).catch((e) => onError(errorText(e)))}>
                    <InsertIcon width={13} height={13} />
                    Вставить
                  </button>
                </div>
              </>
            )}
          </div>
        )}
      </div>
    </div>
  );
}
