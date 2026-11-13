// Annotation model and rendering. Everything is in image pixels, so the
// on-screen canvas and the exported PNG are the very same drawing.
//
// A `rotate` entry turns everything drawn before it (picture, marks, crop)
// by 90°; later entries live in the turned picture's coordinates. The part
// before the last rotation is rasterised once into a "base" and cached.

export type Pt = [number, number];

export type Shape =
  | { t: "pen"; color: string; w: number; pts: Pt[] }
  | { t: "marker"; color: string; w: number; pts: Pt[] }
  | { t: "arrow"; color: string; w: number; a: Pt; b: Pt }
  | { t: "rect"; color: string; w: number; a: Pt; b: Pt }
  | { t: "blur"; w: number; a: Pt; b: Pt }
  /** `font`: letter height in units (see `unitOf`), from FONT_SIZES. */
  | { t: "text"; color: string; font: number; at: Pt; text: string }
  | { t: "step"; color: string; w: number; at: Pt; n: number }
  /** A picture pasted on top (from the clipboard or the shelf). */
  | { t: "image"; src: ImageBitmap; x: number; y: number; width: number; height: number }
  | { t: "crop"; a: Pt; b: Pt }
  | { t: "rotate"; dir: 1 | -1 };

export interface Box {
  x: number;
  y: number;
  w: number;
  h: number;
}

/** Anything with pixels and a size: the original bitmap or a baked canvas. */
export type Base = CanvasImageSource & { width: number; height: number };

export const COLORS = ["#ff4d4f", "#ffc53d", "#52c41a", "#4096ff", "#ffffff", "#141414"];

/** Stroke width range (units) for the slider. */
export const WIDTH_MIN = 1;
export const WIDTH_MAX = 40;
export const DEFAULT_WIDTH = 4;

/** Font sizes offered for text, in units (≈ px on a 1000 px wide image). */
export const FONT_SIZES = [12, 16, 20, 26, 34, 44, 58, 76, 100];
export const DEFAULT_FONT = 26;

/** One "unit" ≈ 1 px on a 1000 px wide image, so marks look the same on any screenshot. */
export function unitOf(w: number, h: number): number {
  return Math.max(1, Math.max(w, h) / 1000);
}

export const stepRadius = (w: number, unit: number) => (9 + w * 1.5) * unit;
const blurBlock = (w: number, unit: number) => (4 + w * 1.4) * unit;

export function box(a: Pt, b: Pt): Box {
  return { x: Math.min(a[0], b[0]), y: Math.min(a[1], b[1]), w: Math.abs(a[0] - b[0]), h: Math.abs(a[1] - b[1]) };
}

export function textPx(font: number, unit: number): number {
  return font * unit;
}

/** Index of the last rotation (−1: none). */
export function lastRotate(shapes: Shape[]): number {
  for (let i = shapes.length - 1; i >= 0; i--) if (shapes[i].t === "rotate") return i;
  return -1;
}

/** The crop of a segment (the last one in it wins), clamped to the picture. */
export function cropOf(shapes: Shape[], w: number, h: number): Box {
  for (let i = shapes.length - 1; i >= 0; i--) {
    const s = shapes[i];
    if (s.t === "rotate") break;
    if (s.t === "crop") {
      const c = box(s.a, s.b);
      const x = Math.max(0, Math.round(c.x));
      const y = Math.max(0, Math.round(c.y));
      return { x, y, w: Math.max(1, Math.min(w - x, Math.round(c.w))), h: Math.max(1, Math.min(h - y, Math.round(c.h))) };
    }
  }
  return { x: 0, y: 0, w, h };
}

const isLight = (color: string) => color === "#ffffff" || color === "#ffc53d";

function smoothPath(ctx: CanvasRenderingContext2D, pts: Pt[]) {
  ctx.beginPath();
  ctx.moveTo(pts[0][0], pts[0][1]);
  if (pts.length === 1) {
    ctx.lineTo(pts[0][0] + 0.01, pts[0][1]);
    return;
  }
  for (let i = 1; i < pts.length - 1; i++) {
    const mx = (pts[i][0] + pts[i + 1][0]) / 2;
    const my = (pts[i][1] + pts[i + 1][1]) / 2;
    ctx.quadraticCurveTo(pts[i][0], pts[i][1], mx, my);
  }
  const last = pts[pts.length - 1];
  ctx.lineTo(last[0], last[1]);
}

function pixelate(ctx: CanvasRenderingContext2D, img: CanvasImageSource, b: Box, block: number) {
  if (b.w < 2 || b.h < 2) return;
  const sw = Math.max(1, Math.round(b.w / block));
  const sh = Math.max(1, Math.round(b.h / block));
  const small = document.createElement("canvas");
  small.width = sw;
  small.height = sh;
  const sctx = small.getContext("2d")!;
  sctx.imageSmoothingEnabled = true;
  sctx.drawImage(img, b.x, b.y, b.w, b.h, 0, 0, sw, sh);
  ctx.save();
  ctx.imageSmoothingEnabled = false;
  ctx.drawImage(small, 0, 0, sw, sh, b.x, b.y, b.w, b.h);
  ctx.restore();
}

function drawShape(ctx: CanvasRenderingContext2D, base: Base, s: Shape, unit: number) {
  ctx.save();
  ctx.lineCap = "round";
  ctx.lineJoin = "round";
  switch (s.t) {
    case "pen":
      ctx.strokeStyle = s.color;
      ctx.lineWidth = s.w * unit;
      smoothPath(ctx, s.pts);
      ctx.stroke();
      break;
    case "marker":
      // One stroke of one path: overlaps inside it do not darken.
      ctx.strokeStyle = s.color;
      ctx.globalAlpha = 0.38;
      ctx.lineCap = "square";
      ctx.lineWidth = s.w * unit * 4;
      smoothPath(ctx, s.pts);
      ctx.stroke();
      break;
    case "arrow": {
      const w = s.w * unit;
      const [ax, ay] = s.a;
      const [bx, by] = s.b;
      const len = Math.hypot(bx - ax, by - ay);
      if (len < 1) break;
      const ux = (bx - ax) / len;
      const uy = (by - ay) / len;
      const head = Math.min(len * 0.6, Math.max(w * 4.5, 10 * unit));
      const half = head * 0.6;
      const baseX = bx - ux * head;
      const baseY = by - uy * head;
      ctx.strokeStyle = s.color;
      ctx.fillStyle = s.color;
      ctx.lineWidth = w;
      ctx.beginPath();
      ctx.moveTo(ax, ay);
      ctx.lineTo(baseX + ux * w * 0.5, baseY + uy * w * 0.5);
      ctx.stroke();
      ctx.beginPath();
      ctx.moveTo(bx, by);
      ctx.lineTo(baseX - uy * half, baseY + ux * half);
      ctx.lineTo(baseX + uy * half, baseY - ux * half);
      ctx.closePath();
      ctx.fill();
      break;
    }
    case "rect": {
      const b = box(s.a, s.b);
      ctx.strokeStyle = s.color;
      ctx.lineWidth = s.w * unit;
      ctx.strokeRect(b.x, b.y, b.w, b.h);
      break;
    }
    case "blur":
      pixelate(ctx, base, box(s.a, s.b), blurBlock(s.w, unit));
      break;
    case "text": {
      const px = s.font * unit;
      ctx.font = `600 ${px}px "Segoe UI", system-ui, sans-serif`;
      ctx.textBaseline = "top";
      ctx.lineWidth = Math.max(2, px * 0.16);
      ctx.strokeStyle = isLight(s.color) ? "rgba(0,0,0,0.75)" : "rgba(255,255,255,0.85)";
      ctx.fillStyle = s.color;
      s.text.split("\n").forEach((line, i) => {
        const y = s.at[1] + i * px * 1.25;
        ctx.strokeText(line, s.at[0], y);
        ctx.fillText(line, s.at[0], y);
      });
      break;
    }
    case "step": {
      const r = stepRadius(s.w, unit);
      ctx.fillStyle = s.color;
      ctx.beginPath();
      ctx.arc(s.at[0], s.at[1], r, 0, Math.PI * 2);
      ctx.fill();
      ctx.fillStyle = isLight(s.color) ? "#141414" : "#ffffff";
      ctx.font = `700 ${r * 1.15}px "Segoe UI", system-ui, sans-serif`;
      ctx.textAlign = "center";
      ctx.textBaseline = "middle";
      ctx.fillText(String(s.n), s.at[0], s.at[1] + r * 0.05);
      break;
    }
    case "image":
      ctx.imageSmoothingQuality = "high";
      ctx.drawImage(s.src, s.x, s.y, s.width, s.height);
      break;
    case "crop":
    case "rotate":
      break;
  }
  ctx.restore();
}

/** A canvas holding `src` turned by 90° (dir 1: clockwise). */
function rotated(src: Base, dir: 1 | -1): HTMLCanvasElement {
  const out = document.createElement("canvas");
  out.width = src.height;
  out.height = src.width;
  const ctx = out.getContext("2d")!;
  if (dir === 1) ctx.setTransform(0, 1, -1, 0, src.height, 0);
  else ctx.setTransform(0, -1, 1, 0, 0, src.width);
  ctx.drawImage(src, 0, 0);
  return out;
}

/**
 * Bakes everything up to and including the last rotation into one picture.
 * Without rotations it is the original image itself.
 */
export function bake(img: Base, shapes: Shape[], unit: number): Base {
  const upto = lastRotate(shapes);
  let base: Base = img;
  let segment: Shape[] = [];
  for (let i = 0; i <= upto; i++) {
    const s = shapes[i];
    if (s.t !== "rotate") {
      segment.push(s);
      continue;
    }
    const flat = document.createElement("canvas");
    render(flat, base, segment, null, unit);
    base = rotated(flat, s.dir);
    segment = [];
  }
  return base;
}

/**
 * Draws `base` and the annotations of the last segment into `canvas`,
 * sized to the crop. `cropDraft` is a crop being dragged: shown as a frame
 * over the uncropped picture instead of being applied.
 */
export function render(canvas: HTMLCanvasElement, base: Base, shapes: Shape[], cropDraft: Box | null, unit: number): Box {
  const crop = cropDraft ? { x: 0, y: 0, w: base.width, h: base.height } : cropOf(shapes, base.width, base.height);
  if (canvas.width !== crop.w) canvas.width = crop.w;
  if (canvas.height !== crop.h) canvas.height = crop.h;
  const ctx = canvas.getContext("2d")!;
  ctx.setTransform(1, 0, 0, 1, -crop.x, -crop.y);
  ctx.clearRect(crop.x, crop.y, crop.w, crop.h);
  ctx.drawImage(base, 0, 0);
  for (const s of shapes) drawShape(ctx, base, s, unit);
  if (cropDraft) {
    ctx.save();
    ctx.fillStyle = "rgba(0,0,0,0.55)";
    ctx.beginPath();
    ctx.rect(0, 0, base.width, base.height);
    ctx.rect(cropDraft.x, cropDraft.y, cropDraft.w, cropDraft.h);
    ctx.fill("evenodd");
    ctx.strokeStyle = "#ffffff";
    ctx.lineWidth = Math.max(1, unit);
    ctx.setLineDash([6 * unit, 4 * unit]);
    ctx.strokeRect(cropDraft.x, cropDraft.y, cropDraft.w, cropDraft.h);
    ctx.restore();
  }
  ctx.setTransform(1, 0, 0, 1, 0, 0);
  return crop;
}

/** Topmost pasted picture under point `p` in the last segment, as an index into `shapes`. */
export function imageAt(shapes: Shape[], p: Pt): number | null {
  for (let i = shapes.length - 1; i > lastRotate(shapes); i--) {
    const s = shapes[i];
    if (s.t === "image" && p[0] >= s.x && p[0] <= s.x + s.width && p[1] >= s.y && p[1] <= s.y + s.height) return i;
  }
  return null;
}
