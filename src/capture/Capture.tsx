// The capture window: a frozen picture of one monitor, covering that monitor.
// The user drags out an area, draws on it with the editor's tools, and sends
// it to the shelf and the clipboard — or reads the text in it. Marks and the
// area live in image pixels and are drawn by the editor's renderer, so the
// result is exactly what was on screen.
import { useCallback, useEffect, useLayoutEffect, useRef, useState, type PointerEvent as ReactPointerEvent } from "react";
import { invoke } from "@tauri-apps/api/core";
import { api, errorText, on } from "../lib/api";
import { framesPresented } from "../lib/frames";
import { animationsOn } from "../lib/motion";
import { COLORS, DEFAULT_FONT, DEFAULT_WIDTH, render, textPx, unitOf, type Box, type Pt, type Shape } from "../editor/draw";
import { ToolIcon } from "../editor/icons";
import { extendMark, isTiny, MARK_TOOLS, nextFont, nextWidth, startMark, textMark, type MarkTool } from "../editor/marks";
import { ToolOptions } from "../editor/ToolOptions";
import { CheckIcon, CloseIcon, ProjectIcon } from "../components/Icons";
import { SendToProject } from "../components/SendToProject";
import { clampBox, moveBox, resizeBox, HANDLES, type Handle } from "./geometry";
import "./capture.css";

/** "select" moves and resizes the area; the rest are the editor's marks. */
type Tool = "select" | MarkTool;

const TOOLS: { id: Tool; key: string; title: string }[] = [
  { id: "select", key: "V", title: "Двигать область" },
  ...MARK_TOOLS,
];

type Drag =
  | { kind: "new"; start: Pt }
  | { kind: "move"; start: Pt; orig: Box }
  | { kind: "resize"; handle: Handle; orig: Box }
  | { kind: "draw" };

const GAP = 8;
/** Where the notch's capsule sits: the top centre, its middle this low (CSS px). */
const CAPSULE_Y = 16;
const CAPSULE_W = 140;

export function Capture() {
  const [img, setImg] = useState<ImageBitmap | null>(null);
  const [sel, setSel] = useState<Box | null>(null);
  const [shapes, setShapes] = useState<Shape[]>([]);
  const [redo, setRedo] = useState<Shape[]>([]);
  const [tool, setTool] = useState<Tool>("select");
  const [color, setColor] = useState(COLORS[0]);
  const [width, setWidth] = useState(DEFAULT_WIDTH);
  const [font, setFont] = useState(DEFAULT_FONT);
  const [text, setText] = useState<{ at: Pt; value: string } | null>(null);
  const [dragging, setDragging] = useState(false);
  const [busy, setBusy] = useState(false);
  const [status, setStatus] = useState<string | null>(null);
  // «В проект…» is open under the bar.
  const [papka, setPapka] = useState(false);
  const [view, setView] = useState({ w: window.innerWidth, h: window.innerHeight });
  const [bar, setBar] = useState({ w: 0, h: 0 });
  // «Готово»: the picture is flying into the notch, the rest steps aside.
  const [flying, setFlying] = useState(false);

  const root = useRef<HTMLDivElement>(null);
  const canvas = useRef<HTMLCanvasElement>(null);
  // The dimming with a hole for the area, moved directly during a drag.
  const hole = useRef<HTMLDivElement>(null);
  const barRef = useRef<HTMLDivElement>(null);
  const textField = useRef<HTMLTextAreaElement>(null);
  const drag = useRef<Drag | null>(null);
  const draft = useRef<Shape | null>(null);
  const live = useRef<Box | null>(null);
  const frame = useRef(0);
  // A new picture was drawn and waits to be shown.
  const pendingShow = useRef(false);
  const loadSeq = useRef(0);
  const bitmapRef = useRef<ImageBitmap | null>(null);

  const unit = img ? unitOf(img.width, img.height) : 1;
  /** Image pixels per CSS pixel. */
  const k = img ? img.width / view.w : 1;

  // ---- loading -----------------------------------------------------------
  const load = useCallback(async () => {
    const seq = ++loadSeq.current;
    try {
      const raw = new Uint8Array(await api.captureImage());
      const header = new DataView(raw.buffer, raw.byteOffset, 8);
      const w = header.getUint32(0, true);
      const h = header.getUint32(4, true);
      const pixels = new ImageData(new Uint8ClampedArray(raw.buffer, raw.byteOffset + 8, w * h * 4), w, h);
      const bitmap = await createImageBitmap(pixels);
      if (seq !== loadSeq.current) {
        bitmap.close();
        return;
      }
      drag.current = null;
      draft.current = null;
      live.current = null;
      pendingShow.current = true;
      setSel(null);
      setShapes([]);
      setRedo([]);
      setTool("select");
      setText(null);
      setStatus(null);
      setBusy(false);
      setDragging(false);
      setFlying(false);
      setPapka(false);
      root.current?.querySelectorAll(".cap-fly").forEach((el) => el.remove());
      setView({ w: window.innerWidth, h: window.innerHeight });
      // A 4K screen is ~33 MB of pixels: free the previous one right away.
      bitmapRef.current?.close();
      bitmapRef.current = bitmap;
      setImg(bitmap);
    } catch {
      // Nothing to show: let the next PrintScreen start afresh.
      if (seq === loadSeq.current) void api.captureClose();
    }
  }, []);

  useEffect(() => {
    void load();
    const unlisten = on<null>("capture-open", () => void load());
    const onResize = () => setView({ w: window.innerWidth, h: window.innerHeight });
    window.addEventListener("resize", onResize);
    return () => {
      ++loadSeq.current;
      void unlisten.then((off) => off());
      window.removeEventListener("resize", onResize);
    };
  }, [load]);

  // ---- drawing -----------------------------------------------------------
  const draw = useCallback(() => {
    if (!img || !canvas.current) return;
    const d = draft.current;
    // The canvas holds the screen and the marks; the dimming and the frame
    // are a layer above it, so they can fade.
    // Flying: the marks left with the picture; what stays looks like the
    // live screen.
    render(canvas.current, img, flying ? [] : d ? [...shapes, d] : shapes, null, unit);
    const el = hole.current;
    if (el) {
      const a = live.current;
      const scale = img.width / window.innerWidth;
      el.style.left = `${a ? a.x / scale : 0}px`;
      el.style.top = `${a ? a.y / scale : 0}px`;
      el.style.width = `${a ? a.w / scale : 0}px`;
      el.style.height = `${a ? a.h / scale : 0}px`;
      el.classList.toggle("framed", !!a);
    }
  }, [img, shapes, unit, flying]);

  // The newest `draw`: a frame queued during a drag must not paint with the
  // marks of an older render (the finished mark would vanish until the next
  // redraw).
  const drawLatest = useRef(draw);
  drawLatest.current = draw;

  /** During a drag: at most one redraw per frame. */
  const schedule = () => {
    cancelAnimationFrame(frame.current);
    frame.current = requestAnimationFrame(() => drawLatest.current());
  };

  useLayoutEffect(() => {
    cancelAnimationFrame(frame.current);
    live.current = sel;
    draw();
  }, [draw, sel]);

  // A new picture is on the canvas: once it has really been presented, the
  // (cloaked) window may appear — never with the previous capture in it.
  useEffect(() => {
    if (!img || !pendingShow.current) return;
    pendingShow.current = false;
    const controller = new AbortController();
    void framesPresented(2_000, 3, { requireFrames: true, signal: controller.signal })
      .then(() => api.captureShow())
      .catch(() => {
        if (!controller.signal.aborted) void api.captureClose();
      });
    return () => controller.abort();
  }, [img]);

  // ---- marks -------------------------------------------------------------
  const add = (s: Shape) => {
    setShapes((list) => [...list, s]);
    setRedo([]);
  };
  const undo = () => {
    const last = shapes[shapes.length - 1];
    if (!last) return;
    setShapes(shapes.slice(0, -1));
    setRedo((r) => [last, ...r]);
  };
  const redoOne = () => {
    const next = redo[0];
    if (!next) return;
    setShapes([...shapes, next]);
    setRedo(redo.slice(1));
  };

  const commitText = () => {
    const mark = text && textMark(text.value, text.at, color, font);
    if (mark) add(mark);
    setText(null);
  };

  // ---- pointer -----------------------------------------------------------
  const toImage = (e: { clientX: number; clientY: number }): Pt => [e.clientX * k, e.clientY * k];
  const inside = (b: Box, p: Pt) => p[0] >= b.x && p[0] <= b.x + b.w && p[1] >= b.y && p[1] <= b.y + b.h;

  const onPointerDown = (e: ReactPointerEvent) => {
    if (e.button !== 0 || !img || busy) return;
    if (text) {
      commitText();
      return;
    }
    const p = toImage(e);
    const area = live.current;
    if (!area || tool === "select") {
      drag.current = area && inside(area, p) ? { kind: "move", start: p, orig: area } : { kind: "new", start: p };
    } else if (tool === "text") {
      // The new text field takes the focus, not the page.
      e.preventDefault();
      setText({ at: p, value: "" });
      return;
    } else if (tool === "step") {
      add(startMark("step", p, color, width, shapes)!);
      return;
    } else {
      drag.current = { kind: "draw" };
      draft.current = startMark(tool, p, color, width, shapes);
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
      if (draft.current) extendMark(draft.current, p, e.shiftKey);
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
    cancelAnimationFrame(frame.current);
    setDragging(false);
    if (!d) return;
    if (d.kind === "draw") {
      const s = draft.current;
      draft.current = null;
      if (s && !isTiny(s)) add(s);
      else draw();
      return;
    }
    // A click without a drag is not an area.
    const area = live.current;
    const next = area && area.w >= 4 && area.h >= 4 ? area : d.kind === "new" ? null : sel;
    live.current = next;
    setSel(next);
    draw();
    // A fresh area: ready to draw on it right away.
    if (d.kind === "new" && next && !sel) setTool("arrow");
  };

  // ---- output ------------------------------------------------------------
  /** The area (the whole screen without one) with `marks` on it. */
  const picture = (marks: Shape[]): HTMLCanvasElement => {
    if (!img) throw new Error("снимок не загружен");
    const a = sel ?? { x: 0, y: 0, w: img.width, h: img.height };
    const crop: Shape = { t: "crop", a: [a.x, a.y], b: [a.x + a.w, a.y + a.h] };
    const out = document.createElement("canvas");
    render(out, img, [...marks, crop], null, unit);
    return out;
  };

  const pngOf = async (out: HTMLCanvasElement): Promise<Uint8Array> => {
    const blob = await new Promise<Blob | null>((ok) => out.toBlob(ok, "image/png"));
    if (!blob) throw new Error("не удалось собрать PNG");
    return new Uint8Array(await blob.arrayBuffer());
  };

  const exportPng = (marks: Shape[]) => pngOf(picture(marks));

  /**
   * The finished picture shrinks from its place into the notch at the top
   * centre, while the dimming and the tools step aside.
   */
  const flyToNotch = async (out: HTMLCanvasElement) => {
    if (!img || !animationsOn() || !root.current) return;
    const a = sel ?? { x: 0, y: 0, w: img.width, h: img.height };
    const box = { x: a.x / k, y: a.y / k, w: a.w / k, h: a.h / k };
    out.className = "cap-fly";
    Object.assign(out.style, { left: `${box.x}px`, top: `${box.y}px`, width: `${box.w}px`, height: `${box.h}px` });
    root.current.append(out);
    setFlying(true);
    const scale = Math.min(1, CAPSULE_W / box.w, (CAPSULE_W * 0.6) / box.h);
    const dx = view.w / 2 - (box.x + box.w / 2);
    const dy = CAPSULE_Y - (box.y + box.h / 2);
    await out.animate(
      [
        { transform: "translate(0, 0) scale(1)", opacity: 1, borderRadius: "0px" },
        { opacity: 1, offset: 0.7 },
        { transform: `translate(${dx}px, ${dy}px) scale(${scale})`, opacity: 0, borderRadius: `${12 / scale}px` },
      ],
      { duration: 300, easing: "cubic-bezier(0.4, 0, 0.2, 1)", fill: "forwards" },
    ).finished;
  };

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

  /** To the shelf and the clipboard. The window goes first: it is done. */
  const done = () =>
    run(async () => {
      // Text still being typed belongs to the picture too.
      const typed = text && textMark(text.value, text.at, color, font);
      const out = picture(typed ? [...shapes, typed] : shapes);
      const png = await pngOf(out);
      setText(null);
      await flyToNotch(out);
      await api.captureClose();
      // A full-screen picture is tens of MB: not kept until the next capture.
      out.remove();
      // After the window is gone, so the notch is seen taking the picture.
      await invoke("editor_result", png, { headers: { "x-notch-action": "save", "x-notch-source": "" } });
    });

  /** «В проект…»: the picture as «Готово» would make it, into a Papka
   *  project's inbox. The backend closes the window once it is written. */
  const sendToProject = async (project: string, caption: string) => {
    // Busy like «Готово»: no second way out while this one is on its way.
    setBusy(true);
    try {
      const typed = text && textMark(text.value, text.at, color, font);
      const png = await exportPng(typed ? [...shapes, typed] : shapes);
      return await api.papkaSendCapture(png, project, caption);
    } catch (e) {
      setBusy(false);
      throw e;
    }
  };

  /** Reads the text in the area; `translate` sends it to the translator. */
  const readText = (translate: boolean) =>
    run(async () => {
      const { text: found } = await api.ocrPng(await exportPng([]));
      await api.captureText(found, translate);
    });

  const cancel = () => void api.captureClose();

  // ---- keyboard ----------------------------------------------------------
  const keys = useRef({ done, cancel, undo, redoOne, img, tool });
  keys.current = { done, cancel, undo, redoOne, img, tool };
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      // The text field has its own keys (Enter, Esc).
      if ((e.target as HTMLElement).closest?.("textarea, select, input")) return;
      const { done, cancel, undo, redoOne, img, tool } = keys.current;
      const ctrl = e.ctrlKey || e.metaKey;
      if (e.key === "Escape") cancel();
      else if (e.key === "Enter" || (ctrl && (e.code === "KeyC" || e.code === "KeyS"))) void done();
      else if (ctrl && e.code === "KeyZ") (e.shiftKey ? redoOne : undo)();
      else if (ctrl && e.code === "KeyY") redoOne();
      else if (ctrl && e.code === "KeyA" && img) setSel({ x: 0, y: 0, w: img.width, h: img.height });
      else if (!ctrl && !e.altKey && (e.code === "BracketLeft" || e.code === "BracketRight")) {
        const dir = e.code === "BracketRight" ? 1 : -1;
        if (tool === "text") setFont((f) => nextFont(f, dir));
        else setWidth((w) => nextWidth(w, dir));
      } else if (!ctrl && !e.altKey) {
        const t = TOOLS.find((x) => `Key${x.key}` === e.code);
        if (t) setTool(t.id);
        else if (e.code >= "Digit1" && e.code <= "Digit6") setColor(COLORS[Number(e.code.slice(5)) - 1]);
        else return;
      } else return;
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
  // The bar stays mounted while there is an area (it rises in once) and only
  // steps aside while dragging or flying.
  const showBar = !!area;
  const barAway = dragging || flying;

  useLayoutEffect(() => {
    const el = barRef.current;
    if (!el) return;
    const w = el.offsetWidth;
    const h = el.offsetHeight;
    setBar((b) => (b.w === w && b.h === h ? b : { w, h }));
  });

  let barLeft = 0;
  let barTop = 0;
  if (area) {
    const right = area.x + area.w;
    const bottom = area.y + area.h;
    // Under the area; above it at the bottom of the screen; inside it when
    // the area fills the screen.
    if (bottom + GAP + bar.h <= view.h) barTop = bottom + GAP;
    else if (area.y - GAP - bar.h >= 0) barTop = area.y - GAP - bar.h;
    else barTop = bottom - GAP - bar.h;
    barLeft = Math.max(GAP, Math.min(right - bar.w, view.w - bar.w - GAP));
  }
  const labelTop = area && area.y >= 26 ? area.y - 24 : (area?.y ?? 0) + 6;

  return (
    <div
      ref={root}
      className={`capture tool-${tool}`}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
      onPointerCancel={onPointerUp}
    >
      <canvas ref={canvas} className="cap-canvas" />
      <div className={`cap-dim${flying ? " away" : ""}`}>
        <div ref={hole} className="cap-hole" />
      </div>

      {!area && !dragging && img && !flying && (
        <div className="cap-hint">Выделите область · Enter — весь экран · Esc — отмена</div>
      )}

      {area && !dragging && !flying && (
        <div
          className={`cap-area${tool === "select" ? " movable" : ""}`}
          style={{ left: area.x, top: area.y, width: area.w, height: area.h }}
        >
          {HANDLES.map((h) => (
            <span key={h} className={`cap-handle h-${h}`} onPointerDown={(e) => startResize(e, h)} />
          ))}
        </div>
      )}

      {sel && area && !flying && (
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
            fontSize: textPx(font, unit) / k,
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
          onBlur={commitText}
        />
      )}

      {showBar && (
        <div
          ref={barRef}
          className={`cap-bar${barAway ? " away" : ""}`}
          style={{ left: barLeft, top: barTop, maxWidth: view.w - GAP * 2 }}
          onPointerDown={(e) => e.stopPropagation()}
        >
          <div className="ed-group">
            {TOOLS.map((t) => (
              <button
                key={t.id}
                className={`ed-tool${tool === t.id ? " active" : ""}`}
                title={`${t.title} (${t.key})`}
                onClick={() => {
                  if (text) commitText();
                  setTool(t.id);
                }}
              >
                <ToolIcon tool={t.id} />
              </button>
            ))}
          </div>
          <span className="ed-sep" />
          <ToolOptions tool={tool} color={color} onColor={setColor} width={width} onWidth={setWidth} font={font} onFont={setFont} />
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
            <button className="btn" disabled={busy} title="Распознать текст в области и положить его в буфер" onClick={() => void readText(false)}>
              Текст
            </button>
            <button className="btn" disabled={busy} title="Распознать текст в области и перевести" onClick={() => void readText(true)}>
              Перевести
            </button>
            <button className="btn" disabled={busy || !img} title="Сохранить снимок с подписью во «Входящие» проекта Papka" onClick={() => setPapka((open) => !open)}>
              <ProjectIcon width={14} height={14} />
              В проект…
            </button>
            <button className="btn primary" disabled={busy} title="На полку и в буфер обмена (Enter)" onClick={() => void done()}>
              <CheckIcon width={14} height={14} />
              Готово
            </button>
            <button className="ed-tool" title="Отмена (Esc)" onClick={cancel}>
              <CloseIcon width={14} height={14} />
            </button>
          </div>
        </div>
      )}

      {showBar && papka && !barAway && (
        <div
          className="cap-papka"
          style={{
            left: Math.max(GAP, Math.min(barLeft + bar.w - 380, view.w - 380 - GAP)),
            // Under the bar when there is room, otherwise above it.
            top: barTop + bar.h + GAP + 220 <= view.h ? barTop + bar.h + GAP : Math.max(GAP, barTop - GAP - 220),
          }}
        >
          <SendToProject what="снимок" send={sendToProject} onDone={() => setPapka(false)} onClose={() => setPapka(false)} />
        </div>
      )}

      {(status || (busy && !flying)) && <div className={`cap-status${status ? " error" : ""}`}>{status ?? "Секунду…"}</div>}
    </div>
  );
}
