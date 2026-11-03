import type { SVGProps } from "react";

const base = (props: SVGProps<SVGSVGElement>) => ({
  width: 16,
  height: 16,
  viewBox: "0 0 24 24",
  fill: "none",
  stroke: "currentColor",
  strokeWidth: 1.8,
  strokeLinecap: "round" as const,
  strokeLinejoin: "round" as const,
  "aria-hidden": true,
  ...props,
});

export const BackIcon = (p: SVGProps<SVGSVGElement>) => (
  <svg {...base(p)}>
    <path d="M15 5l-7 7 7 7" />
  </svg>
);

export const UngroupIcon = (p: SVGProps<SVGSVGElement>) => (
  <svg {...base(p)}>
    <rect x="3" y="8" width="11" height="11" rx="2.5" />
    <path d="M10 4h8.5A2.5 2.5 0 0 1 21 6.5V15M16 3l2 1-2 1.5" />
  </svg>
);

/** Arrow into a text caret: "paste into the active field". */
export const InsertIcon = (p: SVGProps<SVGSVGElement>) => (
  <svg {...base(p)}>
    <path d="M4 12h10M10 8l4 4-4 4M19 5v14" />
  </svg>
);
