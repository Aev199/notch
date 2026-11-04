import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { api, direction, errorText, on, type Provider, type Translation } from "../lib/api";
import { translate } from "../lib/translate";
import { isSuperseded } from "../lib/offline";
import { COLORS, box, cropOf, fontPx, render, unitOf, type Box, type Pt, type Shape, type Size } from "./draw";
import { ToolIcon } from "./icons";
import { CloseIcon } from "../components/Icons";

export type Tool = "pen" | "marker" | "arrow" | "rect" | "text" | "step" | "blur" | "crop";

const TOOLS: { id: Tool; key: string; title: string }[] = [
  { id: "pen", key: "P", title: "Кисть" },
  { id: "marker", key: "M", title: "Маркер" },
  { id: "arrow", key: "A", title: "Стрелка" },
  { id: "rect", key: "R", title: "Рамка" },
  { id: "text", key: "T", title: "Текст" },
  { id: "step", key: "N", title: "Номер шага" },
  { id: "blur", key: "B", title: "Скрыть (пикселизация)" },
  { id: "crop", key: "C", title: "Обрезать" },
];

const SIZES: { id: Size; title: string }[] = [
  { id: 0, title: "Тонко" },
  { id: 1, title: "Средне" },
  { id: 2, title: "Толсто" },
];

type Job<T> = { state: "idle" } | { state: "busy" } | { state: "done"; value: T } | { state: "error"; message: string };

interface TextDraft {
  at: Pt;
  value: string;
}

export function Editor({ initialId }: { initialId: string }) {
  const [id, setId] = useState(initialId);
  const [img, setImg] = useState<ImageBitmap | null>(null);
  const [shapes, setShapes] = useState<Shape[]>([]);
  const [redo, setRedo] = useState<Shape[]>([]);
  const [tool, setTool] = useState<Tool>("arrow");
  const [color, setColor] = useState(COLORS[0]);
  const [size, setSize] = useState<Size>(1);
  const [text, setText] = useState<TextDraft | null>(null);
  const [status, setStatus] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  // Side panel: text found on the picture and its translation.
  const [panel, setPanel] = useState(false);
  const [ocrText, setOcrText] = useState("");
  const [ocr, setOcr] = useState<Job<null>>({ state: "idle" });
  const [tr, setTr] = useState<Job<Translation>>({ state: "idle" });
  const [provider, setProvider] = useState<Provider>("offline");
  const ocrCrop = useRef<string>("");
  const trSeq = useRef(0);
  const trTimer = useRef<number>();

  const canvas = useRef<HTMLCanvasElement>(null);
  const textField = useRef<HTMLTextAreaElement>(null);
  const draft = useRef<Shape | null>(null);
  const crop = useRef<Box>({ x: 0, y: 0, w: 1, h: 1 });
  const frame = useRef(0);
  const dirty = shapes.length > 0;
  // Read by long-lived listeners without re-subscribing them.
  const dirtyRef = useRef(false);
  dirtyRef.current = dirty;

  // ---- loading -----------------------------------------------------------
  const load = useCallback(async (shotId: string) => {
    try {
      const bytes = await api.shelfImage(shotId);
      const bitmap = await createImageBitmap(new Blob([bytes], { type: "image/png" }));
      setImg(bitmap);
      setShapes([]);
      setRedo([]);
      setText(null);
      setId(shotId);
      setPanel(false);
      setOcr({ state: "idle" });
      setTr({ state: "idle" });
      setOcrText("");
    } catch (e) {
      setStatus(errorText(e));
    }
  }, []);

  useEffect(() => {
    void api.snapshot().then((snap) => setProvider(snap.settings.provider)).catch(() => {});
  }, []);

  useEffect(() => {
    void load(initialId);
    const unlisten = on<string>("editor-open", (next) => {
      if (!dirtyRef.current || window.confirm("Отменить правки текущего скриншота?")) void load(next);
    });
    const unclose = getCurrentWindow().onCloseRequested((e) => {
      if (dirtyRef.current && !window.confirm("Закрыть без сохранения?")) e.preventDefault();
    });
    return () => {
      void unlisten.then((off) => off());
      void unclose.then((off) => off());
    };
  }, [initialId, load]);

  // ---- drawing -----------------------------------------------------------
  const redraw = useCallback(() => {
    cancelAnimationFrame(frame.current);
    frame.current = requestAnimationFrame(() => {
      if (!img || !canvas.current) return;
      const d = draft.current;
      const list = d && d.t !== "crop" ? [...shapes, d] : shapes;
      // With the crop tool the whole picture is shown, the current crop as a frame.
      let cropDraft = d && d.t === "crop" ? box(d.a, d.b) : null;
      if (!cropDraft && tool === "crop" && shapes.some((x) => x.t === "crop")) {
        cropDraft = cropOf(shapes, img.width, img.height);
      }
      if (!cropDraft && tool === "crop") cropDraft = { x: 0, y: 0, w: img.width, h: img.height };
      crop.current = render(canvas.current, img, list, cropDraft);
    });
  }, [img, shapes, tool]);

  useLayoutEffect(redraw, [redraw]);

  const toImage = (e: { clientX: number; clientY: number }): Pt => {
    const r = canvas.current!.getBoundingClientRect();
    const c = crop.current;
    return [((e.clientX - r.left) / r.width) * c.w + c.x, ((e.clientY - r.top) / r.height) * c.h + c.y];
  };

  const commit = (s: Shape) => {
    setShapes((list) => [...list, s]);
    setRedo([]);
  };

  const onPointerDown = (e: React.PointerEvent<HTMLCanvasElement>) => {
    if (e.button !== 0 || !img) return;
    if (text) {
      commitText();
      return;
    }
    const p = toImage(e);
    if (tool === "text") {
      // Keep the browser from moving focus to the canvas after this event;
      // the new text field takes it (see the effect below).
      e.preventDefault();
      setText({ at: p, value: "" });
      return;
    }
    if (tool === "step") {
      commit({ t: "step", color, size, at: p, n: shapes.filter((s) => s.t === "step").length + 1 });
      return;
    }
    e.currentTarget.setPointerCapture(e.pointerId);
    if (tool === "pen" || tool === "marker") draft.current = { t: tool, color, size, pts: [p] };
    else if (tool === "arrow" || tool === "rect") draft.current = { t: tool, color, size, a: p, b: p };
    else if (tool === "blur") draft.current = { t: "blur", size, a: p, b: p };
    else if (tool === "crop") draft.current = { t: "crop", a: p, b: p };
    redraw();
  };

  const onPointerMove = (e: React.PointerEvent<HTMLCanvasElement>) => {
    const d = draft.current;
    if (!d) return;
    const p = toImage(e);
    if (d.t === "pen" || d.t === "marker") {
      const last = d.pts[d.pts.length - 1];
      if (Math.hypot(p[0] - last[0], p[1] - last[1]) >= 1.5) d.pts.push(p);
    } else if ("b" in d) {
      if (e.shiftKey && d.t === "arrow") {
        // Shift: snap to 45°.
        const dx = p[0] - d.a[0];
        const dy = p[1] - d.a[1];
        const ang = Math.round(Math.atan2(dy, dx) / (Math.PI / 4)) * (Math.PI / 4);
        const len = Math.hypot(dx, dy);
        d.b = [d.a[0] + Math.cos(ang) * len, d.a[1] + Math.sin(ang) * len];
      } else {
        d.b = p;
      }
    }
    redraw();
  };

  const onPointerUp = () => {
    const d = draft.current;
    draft.current = null;
    if (!d) return;
    const tiny = "b" in d && Math.hypot(d.b[0] - d.a[0], d.b[1] - d.a[1]) < 4;
    if (!tiny) commit(d);
    else redraw();
  };

  // ---- text --------------------------------------------------------------
  const commitText = () => {
    if (text && text.value.trim()) commit({ t: "text", color, size, at: text.at, text: text.value.replace(/\s+$/, "") });
    setText(null);
  };

  const textOpen = text !== null;
  useLayoutEffect(() => {
    if (textOpen) textField.current?.focus();
  }, [textOpen]);

  // ---- history & output --------------------------------------------------
  const undo = () => {
    setShapes((list) => {
      if (!list.length) return list;
      setRedo((r) => [list[list.length - 1], ...r]);
      return list.slice(0, -1);
    });
  };
  const redoOne = () => {
    setRedo((r) => {
      if (!r.length) return r;
      setShapes((list) => [...list, r[0]]);
      return r.slice(1);
    });
  };

  /** PNG of the current crop; `bare` leaves the annotations out (for OCR). */
  const exportPng = async (bare = false): Promise<Uint8Array> => {
    if (!img) throw new Error("картинка не загружена");
    const out = document.createElement("canvas");
    render(out, img, bare ? shapes.filter((x) => x.t === "crop") : shapes, null);
    const blob = await new Promise<Blob | null>((ok) => out.toBlob(ok, "image/png"));
    if (!blob) throw new Error("не удалось собрать PNG");
    return new Uint8Array(await blob.arrayBuffer());
  };

  const output = async (action: "save" | "copy") => {
    setBusy(true);
    try {
      const png = await exportPng();
      await invoke("editor_result", png, { headers: { "x-notch-action": action, "x-notch-source": id } });
      if (action === "save") {
        dirtyRef.current = false;
        await getCurrentWindow().close();
      } else {
        setStatus("Скопировано в буфер");
        window.setTimeout(() => setStatus(null), 1500);
      }
    } catch (e) {
      setStatus(errorText(e));
    } finally {
      setBusy(false);
    }
  };

  // ---- text & translation ------------------------------------------------
  const cropKey = () => {
    const c = img ? cropOf(shapes, img.width, img.height) : null;
    return c ? `${c.x},${c.y},${c.w},${c.h}` : "";
  };

  const runTranslate = async (source: string) => {
    window.clearTimeout(trTimer.current);
    if (!source.trim()) {
      setTr({ state: "idle" });
      return;
    }
    const seq = ++trSeq.current;
    setTr({ state: "busy" });
    try {
      const value = await translate(source.trim(), provider);
      if (seq === trSeq.current) setTr({ state: "done", value });
    } catch (e) {
      if (seq === trSeq.current && !isSuperseded(e)) setTr({ state: "error", message: errorText(e) });
    }
  };

  /** Reads the text in the current crop (whole picture without a crop). */
  const recognize = async (): Promise<string | null> => {
    setPanel(true);
    setOcr({ state: "busy" });
    try {
      const { text: found } = await api.ocrPng(await exportPng(true));
      ocrCrop.current = cropKey();
      setOcrText(found);
      setOcr({ state: "done", value: null });
      return found;
    } catch (e) {
      setOcr({ state: "error", message: errorText(e) });
      return null;
    }
  };

  const showText = () => {
    if (panel && ocr.state !== "busy") {
      setPanel(false);
      return;
    }
    if (ocr.state !== "done" || ocrCrop.current !== cropKey()) void recognize();
    else setPanel(true);
  };

  const translateImage = async () => {
    setPanel(true);
    const fresh = ocr.state === "done" && ocrCrop.current === cropKey();
    const source = fresh ? ocrText : await recognize();
    if (source) void runTranslate(source);
  };

  const onOcrEdit = (value: string) => {
    setOcrText(value);
    // Keep the translation in step with hand corrections.
    if (tr.state !== "idle") {
      window.clearTimeout(trTimer.current);
      trTimer.current = window.setTimeout(() => void runTranslate(value), 600);
    }
  };

  const copyText = async (value: string) => {
    try {
      await api.copyText(value);
      setStatus("Текст скопирован");
      window.setTimeout(() => setStatus(null), 1500);
    } catch (e) {
      setStatus(errorText(e));
    }
  };

  const stale = ocr.state === "done" && ocrCrop.current !== cropKey();
  const [from, to] = tr.state === "done" ? [tr.value.from, tr.value.to] : direction(ocrText);

  // ---- keyboard ----------------------------------------------------------
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      // Text fields (annotation text, recognised text) handle their own keys.
      if (text || (e.target as HTMLElement).matches?.("textarea, input")) return;
      const ctrl = e.ctrlKey || e.metaKey;
      if (ctrl && e.code === "KeyZ") {
        e.preventDefault();
        if (e.shiftKey) redoOne();
        else undo();
      } else if (ctrl && e.code === "KeyY") {
        e.preventDefault();
        redoOne();
      } else if (ctrl && e.code === "KeyS") {
        e.preventDefault();
        void output("save");
      } else if (ctrl && e.code === "KeyC") {
        e.preventDefault();
        void output("copy");
      } else if (!ctrl && !e.altKey) {
        const t = TOOLS.find((x) => `Key${x.key}` === e.code);
        if (t) setTool(t.id);
        else if (e.code >= "Digit1" && e.code <= "Digit6") setColor(COLORS[Number(e.code.slice(5)) - 1]);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  // ---- view --------------------------------------------------------------
  const cssScale = () => {
    const c = canvas.current;
    return c && c.width ? c.getBoundingClientRect().width / c.width : 1;
  };
  const textStyle = (): React.CSSProperties => {
    if (!text || !img) return {};
    const r = canvas.current!.getBoundingClientRect();
    const host = canvas.current!.parentElement!.getBoundingClientRect();
    const s = cssScale();
    const c = crop.current;
    const px = fontPx(size, unitOf(img.width, img.height)) * s;
    return {
      left: r.left - host.left + (text.at[0] - c.x) * s,
      top: r.top - host.top + (text.at[1] - c.y) * s,
      fontSize: px,
      lineHeight: 1.25,
      color,
    };
  };
  const cropBox = img ? cropOf(shapes, img.width, img.height) : null;

  return (
    <div className="editor">
      <header className="ed-bar">
        <div className="ed-group">
          {TOOLS.map((t) => (
            <button
              key={t.id}
              className={`ed-tool${tool === t.id ? " active" : ""}`}
              title={`${t.title} (${t.key})`}
              onClick={() => setTool(t.id)}
            >
              <ToolIcon tool={t.id} />
            </button>
          ))}
        </div>
        <span className="ed-sep" />
        <div className="ed-group">
          {COLORS.map((c, i) => (
            <button
              key={c}
              className={`ed-color${color === c ? " active" : ""}`}
              style={{ "--c": c } as React.CSSProperties}
              title={`Цвет (${i + 1})`}
              onClick={() => setColor(c)}
            />
          ))}
        </div>
        <span className="ed-sep" />
        <div className="ed-group">
          {SIZES.map((s) => (
            <button
              key={s.id}
              className={`ed-size${size === s.id ? " active" : ""}`}
              title={s.title}
              onClick={() => setSize(s.id)}
            >
              <i style={{ width: 4 + s.id * 4, height: 4 + s.id * 4 }} />
            </button>
          ))}
        </div>
        <span className="ed-sep" />
        <div className="ed-group">
          <button className="ed-tool" title="Отменить (Ctrl+Z)" disabled={!shapes.length} onClick={undo}>
            <ToolIcon tool="undo" />
          </button>
          <button className="ed-tool" title="Повторить (Ctrl+Y)" disabled={!redo.length} onClick={redoOne}>
            <ToolIcon tool="redo" />
          </button>
        </div>
        <span className="ed-sep" />
        <div className="ed-group">
          <button
            className={`btn${panel ? " on" : ""}`}
            disabled={!img || ocr.state === "busy"}
            onClick={showText}
            title="Распознать текст в рамке обрезки или на всей картинке (офлайн)"
          >
            Текст
          </button>
          <button className="btn" disabled={!img || ocr.state === "busy"} onClick={() => void translateImage()} title="Распознать и перевести">
            Перевести
          </button>
        </div>
        <span className="push" />
        {status && <span className="ed-status">{status}</span>}
        {cropBox && img && (cropBox.w !== img.width || cropBox.h !== img.height) && (
          <span className="ed-status dim">
            {cropBox.w}×{cropBox.h}
          </span>
        )}
        <button className="btn" disabled={busy || !img} onClick={() => void output("copy")} title="Копировать результат (Ctrl+C)">
          Копировать
        </button>
        <button className="btn primary" disabled={busy || !img} onClick={() => void output("save")} title="На полку и в буфер (Ctrl+S)">
          Сохранить
        </button>
      </header>
      <div className="ed-body">
      <main className="ed-stage">
        <canvas
          ref={canvas}
          className={`ed-canvas tool-${tool}`}
          onPointerDown={onPointerDown}
          onPointerMove={onPointerMove}
          onPointerUp={onPointerUp}
          onPointerCancel={() => {
            draft.current = null;
            redraw();
          }}
        />
        {text && (
          <textarea
            ref={textField}
            className="ed-text"
            style={textStyle()}
            value={text.value}
            rows={Math.max(1, text.value.split("\n").length)}
            placeholder="Текст"
            onChange={(e) => setText({ ...text, value: e.target.value })}
            onKeyDown={(e) => {
              if (e.key === "Enter" && !e.shiftKey) {
                e.preventDefault();
                commitText();
              } else if (e.key === "Escape") {
                setText(null);
              }
            }}
            onBlur={commitText}
          />
        )}
      </main>
      {panel && (
        <aside className="ed-panel">
          <section className="ed-panel-part">
            <div className="ed-panel-head">
              <span>Текст на картинке</span>
              {ocr.state === "busy" && <span className="spinner" />}
              {stale && (
                <button className="btn" onClick={() => void recognize()} title="Рамка обрезки изменилась">
                  Распознать заново
                </button>
              )}
              <span className="push" />
              <button className="icon-btn small" title="Скрыть" onClick={() => setPanel(false)}>
                <CloseIcon width={13} height={13} />
              </button>
            </div>
            {ocr.state === "error" ? (
              <p className="error">{ocr.message}</p>
            ) : (
              <textarea
                className="ed-ocr"
                value={ocrText}
                placeholder={ocr.state === "busy" ? "Распознаю…" : "Текст не найден"}
                spellCheck={false}
                onChange={(e) => onOcrEdit(e.target.value)}
              />
            )}
            <div className="ed-panel-actions">
              <button className="btn" disabled={!ocrText.trim()} onClick={() => void copyText(ocrText)}>
                Копировать
              </button>
              <button className="btn" disabled={!ocrText.trim() || tr.state === "busy"} onClick={() => void runTranslate(ocrText)}>
                Перевести
              </button>
            </div>
          </section>
          {tr.state !== "idle" && (
            <section className="ed-panel-part">
              <div className="ed-panel-head">
                <span>
                  {from.toUpperCase()} → {to.toUpperCase()}
                </span>
                {tr.state === "busy" && <span className="spinner" />}
              </div>
              {tr.state === "error" && <p className="error">{tr.message}</p>}
              {tr.state === "done" && <div className="ed-translation">{tr.value.text}</div>}
              <div className="ed-panel-actions">
                <button className="btn" disabled={tr.state !== "done"} onClick={() => tr.state === "done" && void copyText(tr.value.text)}>
                  Копировать перевод
                </button>
              </div>
            </section>
          )}
        </aside>
      )}
      </div>
    </div>
  );
}
