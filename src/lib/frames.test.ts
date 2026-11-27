import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { framesPresented } from "./frames";

describe("window reveal readiness", () => {
  let callbacks: Map<number, FrameRequestCallback>;
  let next: number;

  beforeEach(() => {
    vi.useFakeTimers();
    callbacks = new Map();
    next = 0;
    vi.stubGlobal("window", globalThis);
    vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => {
      callbacks.set(++next, callback);
      return next;
    });
    vi.stubGlobal("cancelAnimationFrame", (id: number) => callbacks.delete(id));
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  const frame = () => {
    const pending = [...callbacks.values()];
    callbacks.clear();
    pending.forEach((callback) => callback(0));
  };

  it("does not reveal a capture when the renderer stalls until the deadline", async () => {
    const ready = framesPresented(2_000, 3, { requireFrames: true });
    const rejected = expect(ready).rejects.toThrow("not painted");
    frame();
    vi.advanceTimersByTime(2_000);
    await rejected;
    expect(callbacks.size).toBe(0);
  });

  it("allows reveal only after all requested frames and clears the deadline", async () => {
    const revealed = vi.fn();
    const ready = framesPresented(2_000, 3, { requireFrames: true }).then(revealed);
    frame();
    frame();
    await Promise.resolve();
    expect(revealed).not.toHaveBeenCalled();
    frame();
    await ready;
    expect(revealed).toHaveBeenCalledOnce();
    expect(vi.getTimerCount()).toBe(0);
  });

  it("cancels an obsolete reveal without leaving a callback for the next capture", async () => {
    const controller = new AbortController();
    const ready = framesPresented(2_000, 3, { requireFrames: true, signal: controller.signal });
    const rejected = expect(ready).rejects.toMatchObject({ name: "AbortError" });
    frame();
    controller.abort();
    await rejected;
    expect(callbacks.size).toBe(0);
    expect(vi.getTimerCount()).toBe(0);
  });

  it("keeps the editor's best-effort timeout", async () => {
    const ready = framesPresented(400, 3);
    vi.advanceTimersByTime(400);
    await expect(ready).resolves.toBeUndefined();
    expect(callbacks.size).toBe(0);
  });
});
