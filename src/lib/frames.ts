/** Waits for frame callbacks; a timeout is not proof that a frame was painted.
 * Capture requires the callbacks; the editor keeps its best-effort fallback. */
export function framesPresented(ms: number, frames = 2, options: { requireFrames?: boolean; signal?: AbortSignal } = {}): Promise<void> {
  return new Promise((resolve, reject) => {
    const { signal } = options;
    let raf = 0;
    const finish = (error?: Error) => {
      window.clearTimeout(timer);
      cancelAnimationFrame(raf);
      signal?.removeEventListener("abort", abort);
      if (error) reject(error);
      else resolve();
    };
    const abort = () => finish(new DOMException("Rendering cancelled", "AbortError"));
    const timer = window.setTimeout(
      () => finish(options.requireFrames ? new Error("Capture frame was not painted") : undefined),
      ms,
    );
    if (signal?.aborted) {
      abort();
      return;
    }
    signal?.addEventListener("abort", abort, { once: true });
    const step = (left: number) =>
      (raf = requestAnimationFrame(() => {
        if (left > 1) return step(left - 1);
        finish();
      }));
    step(frames);
  });
}
