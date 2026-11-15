import { describe, expect, it } from "vitest";
import { box, cropOf, imageAt, lastRotate, unitOf, type Shape } from "./draw";

const image = (x: number, y: number, width: number, height: number): Shape => ({
  t: "image",
  src: {} as ImageBitmap,
  x,
  y,
  width,
  height,
});

describe("geometry", () => {
  it("normalises a box drawn in any direction", () => {
    expect(box([10, 20], [4, 50])).toEqual({ x: 4, y: 20, w: 6, h: 30 });
  });
  it("scales marks with the picture, never below 1", () => {
    expect(unitOf(500, 300)).toBe(1);
    expect(unitOf(3840, 2160)).toBe(3.84);
  });
});

describe("cropOf", () => {
  it("is the whole picture without a crop", () => {
    expect(cropOf([], 800, 600)).toEqual({ x: 0, y: 0, w: 800, h: 600 });
  });
  it("takes the last crop and clamps it to the picture", () => {
    const shapes: Shape[] = [
      { t: "crop", a: [0, 0], b: [10, 10] },
      { t: "crop", a: [700.4, -20], b: [900, 100.6] },
    ];
    expect(cropOf(shapes, 800, 600)).toEqual({ x: 700, y: 0, w: 100, h: 121 });
  });
  it("ignores crops made before the last rotation", () => {
    const shapes: Shape[] = [{ t: "crop", a: [0, 0], b: [10, 10] }, { t: "rotate", dir: 1 }];
    expect(lastRotate(shapes)).toBe(1);
    expect(cropOf(shapes, 600, 800)).toEqual({ x: 0, y: 0, w: 600, h: 800 });
  });
});

describe("imageAt", () => {
  it("finds the topmost pasted picture under the point", () => {
    const shapes: Shape[] = [image(0, 0, 100, 100), image(50, 50, 100, 100)];
    expect(imageAt(shapes, [60, 60])).toBe(1);
    expect(imageAt(shapes, [10, 10])).toBe(0);
    expect(imageAt(shapes, [200, 200])).toBeNull();
  });
  it("cannot pick pictures baked in by a rotation", () => {
    const shapes: Shape[] = [image(0, 0, 100, 100), { t: "rotate", dir: -1 }];
    expect(lastRotate([])).toBe(-1);
    expect(imageAt(shapes, [10, 10])).toBeNull();
  });
});
