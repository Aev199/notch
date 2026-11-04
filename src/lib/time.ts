import { useEffect, useState } from "react";

/** Re-renders the caller every minute while `active` (keeps "5 мин" honest). */
export function useMinuteTick(active: boolean) {
  const [, setTick] = useState(0);
  useEffect(() => {
    if (!active) return;
    const id = window.setInterval(() => setTick((t) => t + 1), 60_000);
    return () => window.clearInterval(id);
  }, [active]);
}
