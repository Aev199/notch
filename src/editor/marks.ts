// The drawing tools shared by the editor and the screen capture window:
// what each one creates on a press and how it follows the pointer.
import { DEFAULT_FONT, FONT_SIZES, WIDTH_MAX, WIDTH_MIN, type Pt, type Shape } from "./draw";

export type MarkTool = "pen" | "marker" | "line" | "arrow" | "rect" | "text" | "step" | "blur";

/** In toolbar order; `key` is the keyboard shortcut. */
export const MARK_TOOLS: { id: MarkTool; key: string; title: string }[] = [
  { id: "pen", key: "P", title: "Кисть" },
  { id: "marker", key: "M", title: "Маркер" },
  { id: "line", key: "L", title: "Линия (Shift — по 45°)" },
  { id: "arrow", key: "A", title: "Стрелка (Shift — по 45°)" },
  { id: "rect", key: "R", title: "Рамка" },
  { id: "text", key: "T", title: "Текст" },
  { id: "step", key: "N", title: "Номер шага" },
  { id: "blur", key: "B", title: "Скрыть (пикселизация)" },
];

/** Tools whose stroke width the slider sets. */
export const STROKE_TOOLS: string[] = ["pen", "marker", "line", "arrow", "rect", "step", "blur"];

/** End point of a line from `a` towards `p`, turned to the nearest 45°. */
export function snap45(a: Pt, p: Pt): Pt {
  const dx = p[0] - a[0];
  const dy = p[1] - a[1];
  const angle = Math.round(Math.atan2(dy, dx) / (Math.PI / 4)) * (Math.PI / 4);
  const length = Math.hypot(dx, dy);
  return [a[0] + Math.cos(angle) * length, a[1] + Math.sin(angle) * length];
}

/**
 * The mark a press at `p` starts. A step number is complete at once; text
 * is typed in a field first, so it starts nothing here (`null`).
 */
export function startMark(tool: MarkTool, p: Pt, color: string, width: number, shapes: Shape[]): Shape | null {
  switch (tool) {
    case "pen":
    case "marker":
      return { t: tool, color, w: width, pts: [p] };
    case "line":
    case "arrow":
    case "rect":
      return { t: tool, color, w: width, a: p, b: p };
    case "blur":
      return { t: "blur", w: width, a: p, b: p };
    case "step":
      return { t: "step", color, w: width, at: p, n: shapes.filter((s) => s.t === "step").length + 1 };
    case "text":
      return null;
  }
}

/** Follows the pointer while a mark is drawn (changes `d` in place). */
export function extendMark(d: Shape, p: Pt, shift: boolean) {
  if (d.t === "pen" || d.t === "marker") {
    const last = d.pts[d.pts.length - 1];
    if (Math.hypot(p[0] - last[0], p[1] - last[1]) >= 1.5) d.pts.push(p);
  } else if (d.t === "line" || d.t === "arrow") {
    d.b = shift ? snap45(d.a, p) : p;
  } else if (d.t === "rect" || d.t === "blur" || d.t === "crop") {
    d.b = p;
  }
}

/** A drag too short to mean anything (a click with a line tool). */
export function isTiny(d: Shape): boolean {
  return "b" in d && "a" in d && Math.hypot(d.b[0] - d.a[0], d.b[1] - d.a[1]) < 4;
}

export const nextWidth = (w: number, dir: 1 | -1) => Math.min(WIDTH_MAX, Math.max(WIDTH_MIN, w + dir));

export function nextFont(font: number, dir: 1 | -1): number {
  const i = Math.max(0, FONT_SIZES.indexOf(font));
  return FONT_SIZES[Math.min(FONT_SIZES.length - 1, Math.max(0, i + dir))];
}

/** A text mark from the typed value, or nothing for blank input. */
export function textMark(value: string, at: Pt, color: string, font: number = DEFAULT_FONT): Shape | null {
  return value.trim() ? { t: "text", color, font, at, text: value.replace(/\s+$/, "") } : null;
}
