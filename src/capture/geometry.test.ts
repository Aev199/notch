import { describe, expect, it } from "vitest";
import { clampBox, moveBox, resizeBox } from "./geometry";
import { snap45 } from "../editor/marks";

const W = 1920;
const H = 1080;

describe("capture area", () => {
  it("turns a box dragged up and to the left upright", () => {
    expect(clampBox({ x: 500, y: 400, w: -200, h: -100 }, W, H)).toEqual({ x: 300, y: 300, w: 200, h: 100 });
  });
  it("stays inside the screen", () => {
    expect(clampBox({ x: -50, y: 1000, w: 300, h: 500 }, W, H)).toEqual({ x: 0, y: 1000, w: 250, h: 80 });
  });
  it("moves without changing size, stopping at the edges", () => {
    const b = { x: 100, y: 100, w: 400, h: 300 };
    expect(moveBox(b, 50, -20, W, H)).toEqual({ x: 150, y: 80, w: 400, h: 300 });
    expect(moveBox(b, 5000, 5000, W, H)).toEqual({ x: 1520, y: 780, w: 400, h: 300 });
    expect(moveBox(b, -5000, -5000, W, H)).toEqual({ x: 0, y: 0, w: 400, h: 300 });
  });
  it("resizes by corners and edges", () => {
    const b = { x: 100, y: 100, w: 400, h: 300 };
    expect(resizeBox(b, "se", [600, 500], W, H)).toEqual({ x: 100, y: 100, w: 500, h: 400 });
    expect(resizeBox(b, "nw", [50, 60], W, H)).toEqual({ x: 50, y: 60, w: 450, h: 340 });
    // An edge handle ignores the other axis.
    expect(resizeBox(b, "e", [700, 999], W, H)).toEqual({ x: 100, y: 100, w: 600, h: 300 });
    expect(resizeBox(b, "n", [999, 50], W, H)).toEqual({ x: 100, y: 50, w: 400, h: 350 });
  });
  it("flips when an edge is dragged past the opposite one", () => {
    const b = { x: 100, y: 100, w: 400, h: 300 };
    expect(resizeBox(b, "w", [700, 0], W, H)).toEqual({ x: 500, y: 100, w: 200, h: 300 });
  });
});

describe("snap45", () => {
  it("keeps the length and turns to the nearest 45°", () => {
    const [x, y] = snap45([0, 0], [100, 8]);
    expect(x).toBeCloseTo(Math.hypot(100, 8));
    expect(y).toBeCloseTo(0);
    const [dx, dy] = snap45([10, 10], [60, 55]);
    expect(dx - 10).toBeCloseTo(dy - 10);
  });
});
