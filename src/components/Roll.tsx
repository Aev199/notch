// A number that rolls when it changes: the old one leaves upwards, the new
// one comes from below (downwards when it decreases).
import { useEffect, useRef } from "react";
import { AnimatePresence, motion } from "motion/react";
import { FADE } from "../lib/anim";

const VARIANTS = {
  enter: (dir: number) => ({ y: dir * 9, opacity: 0 }),
  center: { y: 0, opacity: 1 },
  exit: (dir: number) => ({ y: -dir * 9, opacity: 0 }),
};

export function Roll({ value }: { value: number }) {
  const prev = useRef(value);
  const dir = value >= prev.current ? 1 : -1;
  useEffect(() => {
    prev.current = value;
  }, [value]);
  return (
    <span className="roll">
      <AnimatePresence initial={false} custom={dir}>
        <motion.span key={value} custom={dir} variants={VARIANTS} initial="enter" animate="center" exit="exit" transition={FADE}>
          {value}
        </motion.span>
      </AnimatePresence>
    </span>
  );
}
