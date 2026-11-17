// The app's own "animations" switch. It decides alone: on, animations play
// even when Windows asks for reduced motion; off, there are none — Motion
// gets `reducedMotion`, CSS transitions go through `html.no-anim`.
import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { on, type Settings } from "./api";

let current = true;
const listeners = new Set<(on: boolean) => void>();

function apply(enabled: boolean) {
  current = enabled;
  document.documentElement.classList.toggle("no-anim", !enabled);
  listeners.forEach((l) => l(enabled));
}

let started = false;
function start() {
  if (started) return;
  started = true;
  void invoke<Settings>("get_settings")
    .then((s) => apply(s.animations))
    .catch(() => {});
  void on<Settings>("settings", (s) => apply(s.animations));
}

/** Whether animations are on; for Motion: `reducedMotion={motionMode(on)}`. */
export function useAnimations(): boolean {
  const [enabled, setEnabled] = useState(current);
  useEffect(() => {
    start();
    listeners.add(setEnabled);
    setEnabled(current);
    return () => void listeners.delete(setEnabled);
  }, []);
  return enabled;
}

export const motionMode = (enabled: boolean) => (enabled ? "never" : "always");

/** For imperative animations (Web Animations API): play them or not. */
export const animationsOn = () => current;
