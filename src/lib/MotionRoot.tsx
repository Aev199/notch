import type { ReactNode } from "react";
import { MotionConfig } from "motion/react";
import { motionMode, useAnimations } from "./motion";

/** Every window's root: Motion follows the app's own "animations" switch. */
export function MotionRoot({ children }: { children: ReactNode }) {
  const enabled = useAnimations();
  return <MotionConfig reducedMotion={motionMode(enabled)}>{children}</MotionConfig>;
}
