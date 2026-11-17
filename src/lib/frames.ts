/** Resolves after a few frames have been presented, or after `ms` at most. */
export function framesPresented(ms: number, frames = 2): Promise<void> {
  return new Promise((resolve) => {
    const timer = window.setTimeout(resolve, ms);
    const step = (left: number) =>
      requestAnimationFrame(() => {
        if (left > 1) return step(left - 1);
        window.clearTimeout(timer);
        resolve();
      });
    step(frames);
  });
}
