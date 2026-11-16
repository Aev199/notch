// Geometry of the capture area, in image pixels. Pure functions: tested.
import type { Box, Pt } from "../editor/draw";

export type Handle = "nw" | "n" | "ne" | "e" | "se" | "s" | "sw" | "w";
export const HANDLES: Handle[] = ["nw", "n", "ne", "e", "se", "s", "sw", "w"];

const clamp = (v: number, min: number, max: number) => Math.max(min, Math.min(max, v));

/** A box dragged in any direction, turned upright and kept inside the picture. */
export function clampBox(b: Box, width: number, height: number): Box {
  const x1 = clamp(Math.min(b.x, b.x + b.w), 0, width);
  const y1 = clamp(Math.min(b.y, b.y + b.h), 0, height);
  const x2 = clamp(Math.max(b.x, b.x + b.w), 0, width);
  const y2 = clamp(Math.max(b.y, b.y + b.h), 0, height);
  return { x: x1, y: y1, w: x2 - x1, h: y2 - y1 };
}

/** The box shifted by (dx, dy) without leaving the picture or changing size. */
export function moveBox(b: Box, dx: number, dy: number, width: number, height: number): Box {
  return { x: clamp(b.x + dx, 0, Math.max(0, width - b.w)), y: clamp(b.y + dy, 0, Math.max(0, height - b.h)), w: b.w, h: b.h };
}

/** The box with the edges named by `handle` moved to point `p`. Dragging an
 *  edge past the opposite one flips the box instead of collapsing it. */
export function resizeBox(b: Box, handle: Handle, p: Pt, width: number, height: number): Box {
  let x1 = b.x;
  let y1 = b.y;
  let x2 = b.x + b.w;
  let y2 = b.y + b.h;
  if (handle.includes("w")) x1 = p[0];
  if (handle.includes("e")) x2 = p[0];
  if (handle.includes("n")) y1 = p[1];
  if (handle.includes("s")) y2 = p[1];
  return clampBox({ x: x1, y: y1, w: x2 - x1, h: y2 - y1 }, width, height);
}

