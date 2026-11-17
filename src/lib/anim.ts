// Shared timing of the interface's motion: short, ease-out, no overshoot.
import type { Transition } from "motion/react";

export const EASE = [0.2, 0.8, 0.2, 1] as const;

/** Things finding their new place (a list reordered, an item gone). */
export const MOVE: Transition = { type: "tween", ease: EASE, duration: 0.2 };

/** Things appearing or leaving. */
export const FADE: Transition = { type: "tween", ease: EASE, duration: 0.14 };

/** A list row: arrives from just above, leaves by folding up. */
export const ROW = {
  layout: "position" as const,
  initial: { opacity: 0, y: -6 },
  animate: { opacity: 1, y: 0 },
  exit: { opacity: 0, height: 0, minHeight: 0, paddingTop: 0, paddingBottom: 0, marginTop: 0, overflow: "hidden" },
  transition: { ...FADE, layout: MOVE },
};
