// The capture window: a frozen picture of one monitor, covering that monitor.
// The user drags out an area, may draw on it, and sends it to the shelf and
// the clipboard — or reads the text in it. Marks and the area live in image
// pixels and are drawn by the editor's renderer, so the result is exactly
// what was on screen.
import { useCallback, useEffect, useLayoutEffect, useRef, useState, type PointerEvent as ReactPointerEvent } from "react";
import { invoke } from "@tauri-apps/api/core";
import { api, errorText, on } from "../lib/api";
import { COLORS, DEFAULT_FONT, DEFAULT_WIDTH, render, textPx, unitOf, type Box, type Pt, type Shape } from "../editor/draw";
import { ToolIcon } from "../editor/icons";
import { CheckIcon, CloseIcon } from "../components/Icons";
import { clampBox, moveBox, resizeBox, snap45, HANDLES, type Handle } from "./geometry";
import "./capture.css";

type Tool = "pen" | "line" | "arrow" | "rect" | "marker" | "text";

const TOOLS: { id: Tool; title: string }[] = [
  { id: "pen", title: "Кисть" },
  { id: "line", title: "Линия (Shift — по 45°)" },
  { id: "arrow", title: "Стрелка (Shift — по 45°)" },
  { id: "rect", title: "Рамка" },
  { id: "marker", title: "Маркер" },
  { id: "text", title: "Текст" },
];

type Drag =
  | { kind: "new"; start: Pt }
  | { kind: "move"; start: Pt; orig: Box }
  | { kind: "resize"; handle: Handle; orig: Box }
  | { kind: "draw" };

/** Sizes of the floating bars (CSS px), to keep them on screen. */
const TOOLBAR = { w: 36, h: (TOOLS.length + 2) * 30 + 12 };
const ACTIONS = { w: 330, h: 38 };
const GAP = 8;

export function Capture() {
  const [img, setImg] = useState<ImageBitmap | null>(null);
  const [sel, setSel] = useState<Box | null>(null);
  const [shapes, setShapes] = useState<Shape[]>([]);
  const [tool, setTool] = useState<Tool | null>(null);
  const [color, setColor] = useState(COLORS[0]);
  const [palette, setPalette] = useState(false);
  const [text, setText] = useState<{ at: Pt; value: string } | null>(null);
  const [dragging, setDragging] = useState(false);
  const [busy, setBusy] = useState(false);
  const [status, setStatus] = useState<string | null>(null);
  const [view, setView] = useState({ w: window.innerWidth, h: window.innerHeight });

  const root = useRef<HTMLDivElement>(null);
  const canvas = useRef<HTMLCanvasElement>(null);
  const textField = useRef<HTMLTextAreaElement>(null);
  const drag = useRef<Drag | null>(null);
  const draft = useRef<Shape | null>(null);
  const live = useRef<Box | null>(null);
  const frame = useRef(0);

  const unit = img ? unitOf(img.width, img.height) : 1;
  /** Image pixels per CSS pixel. */
  const k = img ? img.width / view.w : 1;

  // ---- loading -----------------------------------------------------------
  const load = useCallback(async () => {
    try {
      const raw = new Uint8Array(await api.captureImage());
      const header = new DataView(raw.buffer, raw.byteOffset, 8);
      const w = header.getUint32(0, true);
      const h = header.getUint32(4, true);
      const pixels = new ImageData(new Uint8ClampedArray(raw.buffer, raw.byteOffset + 8, w * h * 4), w, h);
      const bitmap = await createImageBitmap(pixels);
      drag.current = null;
      draft.current = null;
      live.current = null;
      setSel(null);
      setShapes([]);
      setTool(null);
      setText(null);
      setPalette(false);
      setStatus(null);
      setBusy(false);
      setDragging(false);
      // A 4K screen is ~33 MB of pixels: free the previous one right away.
      setImg((old) => {
        old?.close();
        return bitmap;
      });
    } catch {
      // Nothing to show: let the next PrintScreen start afresh.
      void api.captureClose();
    }
  }, []);

  useEffect(() => {
    void load();
    const unlisten = on<null>("capture-open", () => void load());
    const onResize = () => setView({ w: window.innerWidth, h: window.innerHeight });
    window.addEventListener("resize", onResize);
    return () => {
      void unlisten.then((off) => off());
      window.removeEventListener("resize", onResize);
    };
  }, [load]);

  // ---- drawing -----------------------------------------------------------
  // Synchronous: the window is hidden while the first frame is drawn, and a
  // hidden window gets no animation frames.
  const draw = useCallback(() => {
    if (!img || !canvas.current) return;
    const d = draft.current;
    const area = live.current ?? { x: 0, y: 0, w: 0, h: 0 };
    render(canvas.current, img, d ? [...shapes, d] : shapes, area, unit);
  }, [img, shapes, unit]);

  useLayoutEffect(() => {
    live.current = sel;
    draw();
  }, [draw, sel]);

  /** During a drag: at most one redraw per frame. */
  const schedule = () => {
    cancelAnimationFrame(frame.current);
    frame.current = requestAnimationFrame(draw);
  };

  // The picture is on the canvas: now the window may appear.
  useEffect(() => {
    if (img) void api.captureShow().catch(() => void api.captureClose());
  }, [img]);

  // ---- pointer -----------------------------------------------------------
  const toImage = (e: { clientX: number; clientY: number }): Pt => [e.clientX * k, e.clientY * k];
  const inside = (b: Box, p: Pt) => p[0] >= b.x && p[0] <= b.x + b.w && p[1] >= b.y && p[1] <= b.y + b.h;

  const commitText = () => {
    if (text && text.value.trim()) {
      setShapes((s) => [...s, { t: "text", color, font: DEFAULT_FONT, at: text.at, text: text.value.replace(/\s+$/, "") }]);
    }
    setText(null);
  };

  const onPointerDown = (e: ReactPointerEvent) => {
    if (e.button !== 0 || !img || busy) return;
    setPalette(false);
    if (text) {
      commitText();
      return;
    }
    const p = toImage(e);
    const area = live.current;
    if (!area || (!tool && !inside(area, p))) {
      drag.current = { kind: "new", start: p };
    } else if (!tool) {
      drag.current = { kind: "move", start: p, orig: area };
    } else if (tool === "text") {
      // The new text field takes the focus, not the canvas.
      e.preventDefault();
      setText({ at: p, value: "" });
      return;
    } else {
      drag.current = { kind: "draw" };
      draft.current =
        tool === "pen" || tool === "marker"
          ? { t: tool, color, w: DEFAULT_WIDTH, pts: [p] }
          : { t: tool, color, w: DEFAULT_WIDTH, a: p, b: p };
    }
    root.current?.setPointerCapture(e.pointerId);
    setDragging(true);
  };

  const startResize = (e: ReactPointerEvent, handle: Handle) => {
    if (e.button !== 0 || !live.current || busy) return;
    e.stopPropagation();
    if (text) commitText();
    drag.current = { kind: "resize", handle, orig: live.current };
    root.current?.setPointerCapture(e.pointerId);
    setDragging(true);
  };

  const onPointerMove = (e: ReactPointerEvent) => {
    const d = drag.current;
    if (!d || !img) return;
    const p = toImage(e);
    if (d.kind === "draw") {
      const s = draft.current;
      if (!s) return;
      if (s.t === "pen" || s.t === "marker") {
        const last = s.pts[s.pts.length - 1];
        if (Math.hypot(p[0] - last[0], p[1] - last[1]) >= 1.5) s.pts.push(p);
      } else if (s.t === "line" || s.t === "arrow") {
        s.b = e.shiftKey ? snap45(s.a, p) : p;
      } else if (s.t === "rect") {
        s.b = p;
      }
    } else if (d.kind === "new") {
      live.current = clampBox({ x: d.start[0], y: d.start[1], w: p[0] - d.start[0], h: p[1] - d.start[1] }, img.width, img.height);
    } else if (d.kind === "move") {
      live.current = moveBox(d.orig, p[0] - d.start[0], p[1] - d.start[1], img.width, img.height);
    } else {
      live.current = resizeBox(d.orig, d.handle, p, img.width, img.height);
    }
    schedule();
  };

  const onPointerUp = () => {
    const d = drag.current;
    drag.current = null;
    setDragging(false);
    if (!d) return;
    if (d.kind === "draw") {
      const s = draft.current;
      draft.current = null;
      const tiny = s && "b" in s && Math.hypot(s.b[0] - s.a[0], s.b[1] - s.a[1]) < 4;
      if (s && !tiny) setShapes((list) => [...list, s]);
      else draw();
      return;
    }
    // A click without a drag is not an area.
    const area = live.current;
    const next = area && area.w >= 4 && area.h >= 4 ? area : d.kind === "new" ? null : sel;
    live.current = next;
    setSel(next);
    draw();
  };

  // ---- output ------------------------------------------------------------
  /** PNG of the area (the whole screen without one); `marks`: what is drawn on it. */
  const exportPng = async (marks: Shape[]): Promise<Uint8Array> => {
    if (!img) throw new Error("снимок не загружен");
    const a = sel ?? { x: 0, y: 0, w: img.width, h: img.height };
    const crop: Shape = { t: "crop", a: [a.x, a.y], b: [a.x + a.w, a.y + a.h] };
    const out = document.createElement("canvas");
    render(out, img, [...marks, crop], null, unit);
    const blob = await new Promise<Blob | null>((ok) => out.toBlob(ok, "image/png"));
    if (!blob) throw new Error("не удалось собрать PNG");
    return new Uint8Array(await blob.arrayBuffer());
  };

  const pendingText = (): Shape[] =>
    text && text.value.trim()
      ? [{ t: "text", color, font: DEFAULT_FONT, at: text.at, text: text.value.replace(/\s+$/, "") }]
      : [];

  const run = async (job: () => Promise<void>) => {
    if (busy || !img) return;
    setBusy(true);
    setStatus(null);
    try {
      await job();
    } catch (e) {
      setStatus(errorText(e));
      setBusy(false);
    }
  };

  /** To the shelf and the clipboard. */
  const done = () =>
    run(async () => {
      // Text still being typed belongs to the picture too.
      const png = await exportPng([...shapes, ...pendingText()]);
      await invoke("editor_result", png, { headers: { "x-notch-action": "save", "x-notch-source": "" } });
      await api.captureClose();
    });

  /** Reads the text in the area; `translate` sends it to the translator. */
  const readText = (translate: boolean) =>
    run(async () => {
      const { text: found } = await api.ocrPng(await exportPng([]));
      await api.captureText(found, translate);
    });

  const cancel = () => void api.captureClose();
  const undo = () => setShapes((list) => list.slice(0, -1));

  // ---- keyboard ----------------------------------------------------------
  const keys = useRef({ done, cancel, undo, img });
  keys.current = { done, cancel, undo, img };
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      // The text field has its own keys (Enter, Esc).
      if ((e.target as HTMLElement).closest?.("textarea")) return;
      const { done, cancel, undo, img } = keys.current;
      const ctrl = e.ctrlKey || e.metaKey;
      if (e.key === "Escape") cancel();
      else if (e.key === "Enter" || (ctrl && (e.code === "KeyC" || e.code === "KeyS"))) void done();
      else if (ctrl && e.code === "KeyZ") undo();
      else if (ctrl && e.code === "KeyA" && img) setSel({ x: 0, y: 0, w: img.width, h: img.height });
      else return;
      e.preventDefault();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  const textOpen = text !== null;
  useLayoutEffect(() => {
    if (textOpen) textField.current?.focus();
  }, [textOpen]);

  // ---- layout of the floating parts (CSS px) -----------------------------
  const area = sel ? { x: sel.x / k, y: sel.y / k, w: sel.w / k, h: sel.h / k } : null;
  const right = area ? area.x + area.w : 0;
  const bottom = area ? area.y + area.h : 0;
  // Tools: to the right of the area, or inside its right edge at the screen's edge.
  const toolsLeft = right + GAP + TOOLBAR.w <= view.w ? right + GAP : right - GAP - TOOLBAR.w;
  const toolsTop = Math.max(GAP, Math.min(bottom - TOOLBAR.h, view.h - TOOLBAR.h - GAP));
  // Actions: under the area, or inside its bottom edge at the screen's edge.
  const below = bottom + GAP + ACTIONS.h <= view.h;
  const actionsTop = below ? bottom + GAP : bottom - GAP - ACTIONS.h;
  const insetRight = toolsLeft < right && !below ? TOOLBAR.w + GAP * 2 : 0;
  const actionsLeft = Math.max(GAP, Math.min(right - ACTIONS.w - insetRight, view.w - ACTIONS.w - GAP));
  const labelTop = area && area.y >= 26 ? area.y - 24 : (area?.y ?? 0) + 6;

  return (
    <div
      ref={root}
      className={`capture${area && !tool ? " can-move" : ""}${tool ? " drawing" : ""}`}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
      onPointerCancel={onPointerUp}
    >
      <canvas ref={canvas} className="cap-canvas" />

      {!area && !dragging && img && (
        <div className="cap-hint">Выделите область · Enter — весь экран · Esc — отмена</div>
      )}

      {area && !dragging && (
        <div
          className={`cap-area${tool ? "" : " movable"}`}
          style={{ left: area.x, top: area.y, width: area.w, height: area.h }}
        >
          {HANDLES.map((h) => (
            <span key={h} className={`cap-handle h-${h}`} onPointerDown={(e) => startResize(e, h)} />
          ))}
        </div>
      )}

      {sel && area && (
        <div className="cap-size" style={{ left: area.x + (area.y >= 26 ? 0 : 6), top: labelTop }}>
          {Math.round(sel.w)}x{Math.round(sel.h)}
        </div>
      )}

      {text && (
        <textarea
          ref={textField}
          className="ed-text"
          style={{
            left: text.at[0] / k,
            top: text.at[1] / k,
            color,
            fontSize: textPx(DEFAULT_FONT, unit) / k,
            lineHeight: 1.25,
          }}
          value={text.value}
          rows={Math.max(1, text.value.split("\n").length)}
          cols={Math.max(6, ...text.value.split("\n").map((l) => l.length + 1))}
          placeholder="Текст"
          onPointerDown={(e) => e.stopPropagation()}
          onChange={(e) => setText({ ...text, value: e.target.value })}
          onKeyDown={(e) => {
            if (e.key === "Enter" && !e.shiftKey) {
              e.preventDefault();
              commitText();
            } else if (e.key === "Escape") {
              setText(null);
            }
          }}
        />
      )}

      {area && !dragging && (
        <>
          <div className="cap-bar vertical" style={{ left: toolsLeft, top: toolsTop }} onPointerDown={(e) => e.stopPropagation()}>
            {TOOLS.map((t) => (
              <button
                key={t.id}
                className={`cap-btn${tool === t.id ? " active" : ""}`}
                title={tool === t.id ? `${t.title} — нажмите ещё раз, чтобы двигать область` : t.title}
                onClick={() => {
                  if (text) commitText();
                  setTool(tool === t.id ? null : t.id);
                }}
              >
                <ToolIcon tool={t.id} />
              </button>
            ))}
            <button className="cap-btn" title="Цвет" onClick={() => setPalette((v) => !v)}>
              <span className="cap-swatch" style={{ background: color }} />
            </button>
            <button className="cap-btn" title="Отменить (Ctrl+Z)" disabled={shapes.length === 0} onClick={undo}>
              <ToolIcon tool="undo" />
            </button>
            {palette && (
              <div className="cap-palette">
                {COLORS.map((c) => (
                  <button
                    key={c}
                    className={`cap-btn${c === color ? " active" : ""}`}
                    onClick={() => {
                      setColor(c);
                      setPalette(false);
                    }}
                  >
                    <span className="cap-swatch" style={{ background: c }} />
                  </button>
                ))}
              </div>
            )}
          </div>

          <div className="cap-bar" style={{ left: actionsLeft, top: actionsTop, width: ACTIONS.w }} onPointerDown={(e) => e.stopPropagation()}>
            <button className="cap-text-btn" disabled={busy} title="Распознать текст в области и положить его в буфер" onClick={() => void readText(false)}>
              Текст
            </button>
            <button className="cap-text-btn" disabled={busy} title="Распознать текст в области и перевести" onClick={() => void readText(true)}>
              Перевести
            </button>
            <span className="cap-push" />
            <button className="cap-text-btn primary" disabled={busy} title="На полку и в буфер обмена (Enter)" onClick={() => void done()}>
              <CheckIcon width={14} height={14} />
              Готово
            </button>
            <button className="cap-btn" title="Отмена (Esc)" onClick={cancel}>
              <CloseIcon width={14} height={14} />
            </button>
          </div>
        </>
      )}

      {(status || busy) && <div className={`cap-status${status ? " error" : ""}`}>{status ?? "Секунду…"}</div>}
    </div>
  );
}
