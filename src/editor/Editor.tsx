import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { api, direction, errorText, on, type Provider, type ShelfItem, type Translation } from "../lib/api";
import { translate } from "../lib/translate";
import { isSuperseded } from "../lib/offline";
import {
  COLORS,
  DEFAULT_FONT,
  DEFAULT_WIDTH,
  FONT_SIZES,
  WIDTH_MAX,
  WIDTH_MIN,
  bake,
  box,
  cropOf,
  imageAt,
  lastRotate,
  render,
  textPx,
  unitOf,
  type Base,
  type Box,
  type Pt,
  type Shape,
} from "./draw";
import { ToolIcon } from "./icons";
import { CloseIcon } from "../components/Icons";

export type Tool = "select" | "pen" | "marker" | "arrow" | "rect" | "text" | "step" | "blur" | "crop";

const TOOLS: { id: Tool; key: string; title: string }[] = [
  { id: "select", key: "V", title: "Выбрать и двигать вставленные картинки" },
  { id: "pen", key: "P", title: "Кисть" },
  { id: "marker", key: "M", title: "Маркер" },
  { id: "arrow", key: "A", title: "Стрелка" },
  { id: "rect", key: "R", title: "Рамка" },
  { id: "text", key: "T", title: "Текст" },
  { id: "step", key: "N", title: "Номер шага" },
  { id: "blur", key: "B", title: "Скрыть (пикселизация)" },
  { id: "crop", key: "C", title: "Обрезать" },
];

/** Tools whose stroke width the slider sets. */
const STROKE_TOOLS: Tool[] = ["pen", "marker", "arrow", "rect", "step", "blur"];

type Job<T> = { state: "idle" } | { state: "busy" } | { state: "done"; value: T } | { state: "error"; message: string };

/** Absolute zoom limits (1 = one image pixel per CSS pixel). */
const MIN_SCALE = 0.05;
const MAX_SCALE = 8;

interface TextDraft {
  at: Pt;
  value: string;
}

/** Moving or resizing a pasted picture. */
interface Grab {
  index: number;
  mode: "move" | "resize";
  start: Pt;
  orig: Extract<Shape, { t: "image" }>;
}

export function Editor({ initialId }: { initialId: string }) {
  const [id, setId] = useState(initialId);
  const [img, setImg] = useState<ImageBitmap | null>(null);
  // History as snapshots: every change (a new mark, a moved picture, a
  // rotation) is a new array; undo/redo swap whole arrays.
  const [shapes, setShapes] = useState<Shape[]>([]);
  const [past, setPast] = useState<Shape[][]>([]);
  const [future, setFuture] = useState<Shape[][]>([]);
  const [tool, setTool] = useState<Tool>("arrow");
  const [color, setColor] = useState(COLORS[0]);
  const [width, setWidth] = useState(DEFAULT_WIDTH);
  const [font, setFont] = useState(DEFAULT_FONT);
  const [selected, setSelected] = useState<number | null>(null);
  const [picker, setPicker] = useState<ShelfItem[] | null>(null);
  // View: `zoom` multiplies the fit-to-window scale; 1 = whole picture visible.
  const [zoom, setZoom] = useState(1);
  const [stageSize, setStageSize] = useState({ w: 0, h: 0 });
  const [paper, setPaper] = useState({ w: 1, h: 1 });
  const stage = useRef<HTMLElement>(null);
  const anchor = useRef<{ fx: number; fy: number; cx: number; cy: number } | null>(null);
  const pan = useRef<{ x: number; y: number; left: number; top: number } | null>(null);
  const [spaceDown, setSpaceDown] = useState(false);
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
  const grab = useRef<Grab | null>(null);
  const crop = useRef<Box>({ x: 0, y: 0, w: 1, h: 1 });
  const frame = useRef(0);

  const unit = img ? unitOf(img.width, img.height) : 1;
  // Everything up to the last rotation, baked once; marks after it go on top.
  const rot = lastRotate(shapes);
  const rotShape = rot >= 0 ? shapes[rot] : null;
  const base: Base | null = useMemo(
    () => (img ? bake(img, shapes.slice(0, rot + 1), unitOf(img.width, img.height)) : null),
    // The prefix only changes when the last rotation itself changes.
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [img, rot, rotShape],
  );
  const segment = useMemo(() => shapes.slice(rot + 1), [shapes, rot]);

  const flash = (message: string) => {
    setStatus(message);
    window.setTimeout(() => setStatus((s) => (s === message ? null : s)), 1600);
  };

  // ---- loading -----------------------------------------------------------
  const load = useCallback(async (shotId: string) => {
    try {
      const bytes = await api.shelfImage(shotId);
      const bitmap = await createImageBitmap(new Blob([bytes], { type: "image/png" }));
      // A 4K screenshot is ~33 MB of pixels: free the previous one right away.
      setImg((old) => {
        old?.close();
        return bitmap;
      });
      setShapes([]);
      setPast([]);
      setFuture([]);
      setSelected(null);
      setText(null);
      setId(shotId);
      setPanel(false);
      setOcr({ state: "idle" });
      setTr({ state: "idle" });
      setOcrText("");
      setZoom(1);
    } catch (e) {
      setStatus(errorText(e));
    }
  }, []);

  useEffect(() => {
    void api.snapshot().then((snap) => setProvider(snap.settings.provider)).catch(() => {});
  }, []);

  useEffect(() => {
    void load(initialId);
    // Another screenshot clicked while the editor is open: just switch.
    // Closing never asks either: unsaved marks are dropped (Ctrl+S saves).
    const unlisten = on<string>("editor-open", (next) => void load(next));
    return () => void unlisten.then((off) => off());
  }, [initialId, load]);

  // ---- history -----------------------------------------------------------
  const commit = (next: Shape[]) => {
    setPast((p) => [...p, shapes]);
    setFuture([]);
    setShapes(next);
  };
  const add = (s: Shape) => commit([...shapes, s]);
  const undo = () => {
    if (!past.length) return;
    setFuture((f) => [shapes, ...f]);
    setShapes(past[past.length - 1]);
    setPast((p) => p.slice(0, -1));
    setSelected(null);
  };
  const redoOne = () => {
    if (!future.length) return;
    setPast((p) => [...p, shapes]);
    setShapes(future[0]);
    setFuture((f) => f.slice(1));
    setSelected(null);
  };

  // ---- drawing -----------------------------------------------------------
  const redraw = useCallback(() => {
    cancelAnimationFrame(frame.current);
    frame.current = requestAnimationFrame(() => {
      if (!base || !canvas.current) return;
      const d = draft.current;
      const list = d && d.t !== "crop" ? [...segment, d] : segment;
      // With the crop tool the whole picture is shown, the current crop as a frame.
      let cropDraft = d && d.t === "crop" ? box(d.a, d.b) : null;
      if (!cropDraft && tool === "crop") cropDraft = cropOf(segment, base.width, base.height);
      const c = render(canvas.current, base, list, cropDraft, unit);
      crop.current = c;
      setPaper((p) => (p.w === c.w && p.h === c.h ? p : { w: c.w, h: c.h }));
    });
  }, [base, segment, tool, unit]);

  useLayoutEffect(redraw, [redraw]);

  const toImage = (e: { clientX: number; clientY: number }): Pt => {
    const r = canvas.current!.getBoundingClientRect();
    const c = crop.current;
    return [((e.clientX - r.left) / r.width) * c.w + c.x, ((e.clientY - r.top) / r.height) * c.h + c.y];
  };

  const onPointerDown = (e: React.PointerEvent<HTMLCanvasElement>) => {
    if (e.button !== 0 || !base) return;
    setPicker(null);
    if (text) {
      commitText();
      return;
    }
    const p = toImage(e);
    if (tool === "select") {
      const hit = imageAt(shapes, p);
      setSelected(hit);
      if (hit !== null) startGrab(e, hit, "move");
      return;
    }
    setSelected(null);
    if (tool === "text") {
      // Keep the browser from moving focus to the canvas after this event;
      // the new text field takes it (see the effect below).
      e.preventDefault();
      setText({ at: p, value: "" });
      return;
    }
    if (tool === "step") {
      add({ t: "step", color, w: width, at: p, n: shapes.filter((s) => s.t === "step").length + 1 });
      return;
    }
    e.currentTarget.setPointerCapture(e.pointerId);
    if (tool === "pen" || tool === "marker") draft.current = { t: tool, color, w: width, pts: [p] };
    else if (tool === "arrow" || tool === "rect") draft.current = { t: tool, color, w: width, a: p, b: p };
    else if (tool === "blur") draft.current = { t: "blur", w: width, a: p, b: p };
    else if (tool === "crop") draft.current = { t: "crop", a: p, b: p };
    redraw();
  };

  const startGrab = (e: React.PointerEvent, index: number, mode: Grab["mode"]) => {
    const s = shapes[index];
    if (s.t !== "image") return;
    e.stopPropagation();
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    grab.current = { index, mode, start: toImage(e), orig: s };
  };

  const onPointerMove = (e: React.PointerEvent) => {
    const g = grab.current;
    if (g) {
      const p = toImage(e);
      const dx = p[0] - g.start[0];
      const dy = p[1] - g.start[1];
      const o = g.orig;
      const next =
        g.mode === "move"
          ? { ...o, x: o.x + dx, y: o.y + dy }
          : (() => {
              // Corner drag keeps the picture's proportions.
              const k = Math.max(0.05, Math.max((o.width + dx) / o.width, (o.height + dy) / o.height));
              return { ...o, width: o.width * k, height: o.height * k };
            })();
      const live = shapes.slice();
      live[g.index] = next;
      setShapes(live);
      return;
    }
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
    const g = grab.current;
    if (g) {
      grab.current = null;
      // The live drag edited `shapes` in place of history; record it once.
      const moved = shapes[g.index];
      if (moved !== g.orig) {
        const before = shapes.slice();
        before[g.index] = g.orig;
        setPast((p) => [...p, before]);
        setFuture([]);
      }
      return;
    }
    const d = draft.current;
    draft.current = null;
    if (!d) return;
    const tiny = "b" in d && Math.hypot(d.b[0] - d.a[0], d.b[1] - d.a[1]) < 4;
    if (!tiny) add(d);
    else redraw();
  };

  // ---- pictures on top -----------------------------------------------------
  /** Puts a picture in the middle of the visible area, at most half its size. */
  const placeImage = (bitmap: ImageBitmap) => {
    const c = crop.current;
    const k = Math.min(1, (c.w * 0.5) / bitmap.width, (c.h * 0.5) / bitmap.height);
    const w = bitmap.width * k;
    const h = bitmap.height * k;
    const s: Shape = { t: "image", src: bitmap, x: c.x + (c.w - w) / 2, y: c.y + (c.h - h) / 2, width: w, height: h };
    commit([...shapes, s]);
    setSelected(shapes.length);
    setTool("select");
  };

  const pasteFromClipboard = async () => {
    try {
      const raw = new Uint8Array(await invoke<ArrayBuffer>("clipboard_image"));
      const view = new DataView(raw.buffer);
      const w = view.getUint32(0, true);
      const h = view.getUint32(4, true);
      const pixels = new ImageData(new Uint8ClampedArray(raw.buffer, 8, w * h * 4), w, h);
      placeImage(await createImageBitmap(pixels));
    } catch (e) {
      flash(errorText(e));
    }
  };

  const openPicker = async () => {
    if (picker) {
      setPicker(null);
      return;
    }
    try {
      const snap = await api.snapshot();
      setPicker(snap.shelf.filter((s) => s.id !== id));
    } catch (e) {
      flash(errorText(e));
    }
  };

  const pasteFromShelf = async (item: ShelfItem) => {
    setPicker(null);
    try {
      const bytes = await api.shelfImage(item.id);
      placeImage(await createImageBitmap(new Blob([bytes], { type: "image/png" })));
    } catch (e) {
      flash(errorText(e));
    }
  };

  const deleteSelected = () => {
    if (selected === null) return;
    commit(shapes.filter((_, i) => i !== selected));
    setSelected(null);
  };

  const rotate = (dir: 1 | -1) => {
    setSelected(null);
    commit([...shapes, { t: "rotate", dir }]);
  };

  // ---- text --------------------------------------------------------------
  const commitText = () => {
    if (text && text.value.trim()) add({ t: "text", color, font, at: text.at, text: text.value.replace(/\s+$/, "") });
    setText(null);
  };

  const textOpen = text !== null;
  useLayoutEffect(() => {
    if (textOpen) textField.current?.focus();
  }, [textOpen]);

  // ---- output ------------------------------------------------------------
  /** PNG of the current crop; `bare` leaves the annotations out (for OCR). */
  const exportPng = async (bare = false): Promise<Uint8Array> => {
    if (!img) throw new Error("картинка не загружена");
    const keep = (s: Shape) => !bare || s.t === "crop" || s.t === "rotate";
    const list = shapes.filter(keep);
    const out = document.createElement("canvas");
    const r = lastRotate(list);
    render(out, bake(img, list.slice(0, r + 1), unit), list.slice(r + 1), null, unit);
    const blob = await new Promise<Blob | null>((ok) => out.toBlob(ok, "image/png"));
    if (!blob) throw new Error("не удалось собрать PNG");
    return new Uint8Array(await blob.arrayBuffer());
  };

  const output = async (action: "save" | "copy") => {
    setBusy(true);
    try {
      const png = await exportPng();
      await invoke("editor_result", png, { headers: { "x-notch-action": action, "x-notch-source": id } });
      if (action === "save") await getCurrentWindow().close();
      else flash("Скопировано в буфер");
    } catch (e) {
      setStatus(errorText(e));
    } finally {
      setBusy(false);
    }
  };

  // ---- text & translation ------------------------------------------------
  const cropKey = () => {
    const c = base ? cropOf(segment, base.width, base.height) : null;
    return c ? `${rot}:${c.x},${c.y},${c.w},${c.h}` : "";
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
      flash("Текст скопирован");
    } catch (e) {
      setStatus(errorText(e));
    }
  };

  const stale = ocr.state === "done" && ocrCrop.current !== cropKey();
  const [from, to] = tr.state === "done" ? [tr.value.from, tr.value.to] : direction(ocrText);

  // ---- zoom & pan --------------------------------------------------------
  useEffect(() => {
    const el = stage.current;
    if (!el) return;
    const ro = new ResizeObserver(() => setStageSize({ w: el.clientWidth, h: el.clientHeight }));
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  const PAD = 32;
  // Fit the picture into the window, never enlarge it (until the stage is measured: 1).
  const fit =
    stageSize.w > PAD && stageSize.h > PAD ? Math.min(1, (stageSize.w - PAD) / paper.w, (stageSize.h - PAD) / paper.h) : 1;
  const scale = Math.min(MAX_SCALE, Math.max(MIN_SCALE, fit * zoom));

  /** Zooms keeping the picture point under (cx, cy) where it is. */
  const zoomAt = (factor: number, cx?: number, cy?: number) => {
    const el = stage.current;
    const cv = canvas.current;
    if (!el || !cv) return;
    const r = cv.getBoundingClientRect();
    const sr = el.getBoundingClientRect();
    const x = cx ?? sr.left + sr.width / 2;
    const y = cy ?? sr.top + sr.height / 2;
    anchor.current = { fx: (x - r.left) / r.width, fy: (y - r.top) / r.height, cx: x - sr.left, cy: y - sr.top };
    setZoom((z) => {
      const target = Math.min(MAX_SCALE, Math.max(MIN_SCALE, fit * z * factor));
      return target / fit;
    });
  };

  useLayoutEffect(() => {
    const a = anchor.current;
    const el = stage.current;
    const cv = canvas.current;
    if (!a || !el || !cv) return;
    anchor.current = null;
    const paperEl = cv.parentElement!;
    el.scrollLeft = paperEl.offsetLeft + a.fx * cv.clientWidth - a.cx;
    el.scrollTop = paperEl.offsetTop + a.fy * cv.clientHeight - a.cy;
  }, [scale]);

  useEffect(() => {
    const el = stage.current;
    if (!el) return;
    const onWheel = (e: WheelEvent) => {
      if (!(e.ctrlKey || e.metaKey)) return; // plain wheel scrolls
      e.preventDefault();
      zoomAt(Math.exp(-e.deltaY * 0.0015), e.clientX, e.clientY);
    };
    el.addEventListener("wheel", onWheel, { passive: false });
    return () => el.removeEventListener("wheel", onWheel);
  });

  // Middle button, or Space held: drag the picture around when zoomed in.
  const onStagePointerDown = (e: React.PointerEvent<HTMLElement>) => {
    if (e.button !== 1 && !(e.button === 0 && spaceDown)) return;
    e.preventDefault();
    e.stopPropagation();
    const el = stage.current!;
    pan.current = { x: e.clientX, y: e.clientY, left: el.scrollLeft, top: el.scrollTop };
    el.setPointerCapture(e.pointerId);
  };
  const onStagePointerMove = (e: React.PointerEvent<HTMLElement>) => {
    const p = pan.current;
    if (!p) return;
    const el = stage.current!;
    el.scrollLeft = p.left - (e.clientX - p.x);
    el.scrollTop = p.top - (e.clientY - p.y);
  };
  const onStagePointerUp = () => (pan.current = null);

  const stepFont = (dir: 1 | -1) => {
    const i = Math.max(0, FONT_SIZES.indexOf(font));
    setFont(FONT_SIZES[Math.min(FONT_SIZES.length - 1, Math.max(0, i + dir))]);
  };
  const stepWidth = (dir: 1 | -1) => setWidth((w) => Math.min(WIDTH_MAX, Math.max(WIDTH_MIN, w + dir)));

  // ---- keyboard ----------------------------------------------------------
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const ctrl = e.ctrlKey || e.metaKey;
      // Zoom works everywhere, also while typing a label.
      if (ctrl && (e.code === "Equal" || e.code === "NumpadAdd")) {
        e.preventDefault();
        zoomAt(1.25);
        return;
      }
      if (ctrl && (e.code === "Minus" || e.code === "NumpadSubtract")) {
        e.preventDefault();
        zoomAt(0.8);
        return;
      }
      if (ctrl && (e.code === "Digit0" || e.code === "Numpad0")) {
        e.preventDefault();
        setZoom(1);
        return;
      }
      // Text fields (annotation text, recognised text) handle their own keys.
      if (text || (e.target as HTMLElement).matches?.("textarea, input")) return;
      if (e.code === "Space") {
        e.preventDefault();
        setSpaceDown(true);
        return;
      }
      if (!ctrl && (e.code === "BracketLeft" || e.code === "BracketRight")) {
        const dir = e.code === "BracketRight" ? 1 : -1;
        if (tool === "text") stepFont(dir);
        else stepWidth(dir);
        return;
      }
      if ((e.code === "Delete" || e.code === "Backspace") && selected !== null) {
        e.preventDefault();
        deleteSelected();
        return;
      }
      if (e.code === "Escape") {
        setSelected(null);
        setPicker(null);
        return;
      }
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
      } else if (ctrl && e.code === "KeyV") {
        e.preventDefault();
        void pasteFromClipboard();
      } else if (!ctrl && !e.altKey) {
        const t = TOOLS.find((x) => `Key${x.key}` === e.code);
        if (t) setTool(t.id);
        else if (e.code >= "Digit1" && e.code <= "Digit6") setColor(COLORS[Number(e.code.slice(5)) - 1]);
      }
    };
    const onKeyUp = (e: KeyboardEvent) => e.code === "Space" && setSpaceDown(false);
    window.addEventListener("keydown", onKey);
    window.addEventListener("keyup", onKeyUp);
    return () => {
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("keyup", onKeyUp);
    };
  });

  // ---- view --------------------------------------------------------------
  const textStyle = (): React.CSSProperties => {
    if (!text || !img) return {};
    const c = crop.current;
    return {
      left: (text.at[0] - c.x) * scale,
      top: (text.at[1] - c.y) * scale,
      fontSize: textPx(font, unit) * scale,
      lineHeight: 1.25,
      color,
    };
  };
  const sel = selected !== null ? shapes[selected] : null;
  const selBox =
    sel && sel.t === "image" && tool !== "crop"
      ? {
          left: (sel.x - crop.current.x) * scale,
          top: (sel.y - crop.current.y) * scale,
          width: sel.width * scale,
          height: sel.height * scale,
        }
      : null;
  const cropBox = base ? cropOf(segment, base.width, base.height) : null;
  const strokeTool = STROKE_TOOLS.includes(tool);

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
        {tool === "text" ? (
          <div className="ed-group ed-font" title="Размер шрифта ([ и ])">
            <button className="ed-tool" onClick={() => stepFont(-1)} disabled={font <= FONT_SIZES[0]} title="Меньше ([)">
              <span className="ed-a small">A</span>
            </button>
            <select value={font} onChange={(e) => setFont(Number(e.target.value))}>
              {FONT_SIZES.map((f) => (
                <option key={f} value={f}>
                  {f}
                </option>
              ))}
            </select>
            <button
              className="ed-tool"
              onClick={() => stepFont(1)}
              disabled={font >= FONT_SIZES[FONT_SIZES.length - 1]}
              title="Больше (])"
            >
              <span className="ed-a">A</span>
            </button>
          </div>
        ) : (
          <label className={`ed-group ed-width${strokeTool ? "" : " off"}`} title="Толщина ([ и ])">
            <i style={{ width: Math.min(18, 2 + width * 0.6), height: Math.min(18, 2 + width * 0.6) }} />
            <input
              type="range"
              min={WIDTH_MIN}
              max={WIDTH_MAX}
              value={width}
              disabled={!strokeTool}
              onChange={(e) => setWidth(Number(e.target.value))}
            />
            <span className="ed-width-value">{width}</span>
          </label>
        )}
        <span className="ed-sep" />
        <div className="ed-group">
          <button className="ed-tool" title="Повернуть влево" disabled={!img} onClick={() => rotate(-1)}>
            <ToolIcon tool="rotl" />
          </button>
          <button className="ed-tool" title="Повернуть вправо" disabled={!img} onClick={() => rotate(1)}>
            <ToolIcon tool="rotr" />
          </button>
          <div className="ed-menu-host">
            <button className={`ed-tool${picker ? " active" : ""}`} title="Вставить картинку с полки" onClick={() => void openPicker()}>
              <ToolIcon tool="layer" />
            </button>
            {picker && (
              <div className="ed-picker">
                <button className="btn" onClick={() => void pasteFromClipboard().then(() => setPicker(null))}>
                  Из буфера обмена (Ctrl+V)
                </button>
                {picker.length === 0 ? (
                  <span className="dim">На полке нет других скриншотов</span>
                ) : (
                  <div className="ed-picker-grid">
                    {picker.map((item) => (
                      <button key={item.id} title={`${item.width}×${item.height}`} onClick={() => void pasteFromShelf(item)}>
                        <img src={item.thumb} alt="" />
                      </button>
                    ))}
                  </div>
                )}
              </div>
            )}
          </div>
        </div>
        <span className="ed-sep" />
        <div className="ed-group">
          <button className="ed-tool" title="Отменить (Ctrl+Z)" disabled={!past.length} onClick={undo}>
            <ToolIcon tool="undo" />
          </button>
          <button className="ed-tool" title="Повторить (Ctrl+Y)" disabled={!future.length} onClick={redoOne}>
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
        {cropBox && base && (cropBox.w !== base.width || cropBox.h !== base.height) && (
          <span className="ed-status dim">
            {cropBox.w}×{cropBox.h}
          </span>
        )}
        <button className="btn" title="По размеру окна (Ctrl+0). Масштаб: Ctrl+колесо" onClick={() => setZoom(1)}>
          {Math.round(scale * 100)}%
        </button>
        <button className="btn" disabled={busy || !img} onClick={() => void output("copy")} title="Копировать результат (Ctrl+C)">
          Копировать
        </button>
        <button
          className="btn primary"
          disabled={busy || !img}
          onClick={() => void output("save")}
          title="На полку и в буфер (Ctrl+S). Без сохранения окно просто закрывается."
        >
          Сохранить
        </button>
      </header>
      <div className="ed-body">
        <main
          ref={stage}
          className={`ed-stage${spaceDown ? " panning" : ""}`}
          onPointerDownCapture={onStagePointerDown}
          onPointerMove={onStagePointerMove}
          onPointerUp={onStagePointerUp}
        >
          <div className="ed-paper" style={{ width: paper.w * scale, height: paper.h * scale }}>
            <canvas
              style={{ width: paper.w * scale, height: paper.h * scale }}
              ref={canvas}
              className={`ed-canvas tool-${tool}${scale > 2 ? " crisp" : ""}`}
              onPointerDown={onPointerDown}
              onPointerMove={onPointerMove}
              onPointerUp={onPointerUp}
              onPointerCancel={() => {
                draft.current = null;
                grab.current = null;
                redraw();
              }}
            />
            {selBox && selected !== null && (
              <div
                className="ed-selection"
                style={selBox}
                onPointerDown={(e) => startGrab(e, selected, "move")}
                onPointerMove={onPointerMove}
                onPointerUp={onPointerUp}
              >
                <span
                  className="ed-handle"
                  title="Потяните, чтобы изменить размер"
                  onPointerDown={(e) => startGrab(e, selected, "resize")}
                />
                <button className="ed-sel-delete" title="Удалить (Delete)" onPointerDown={(e) => e.stopPropagation()} onClick={deleteSelected}>
                  <CloseIcon width={12} height={12} />
                </button>
              </div>
            )}
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
          </div>
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
                  <button
                    className="btn"
                    disabled={tr.state !== "done"}
                    onClick={() => tr.state === "done" && void copyText(tr.value.text)}
                  >
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
