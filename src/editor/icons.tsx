import type { Tool } from "./Editor";

type IconName = Tool | "undo" | "redo" | "rotl" | "rotr" | "layer";

const PATHS: Record<IconName, JSX.Element> = {
  select: <path d="M5 3l14 8-6 1.5L10 19 5 3Z" />,
  pen: <path d="M4 20l4-1 11-11-3-3L5 16l-1 4ZM14 7l3 3" />,
  marker: <path d="M6 15l7-7 4 4-7 7H6v-4ZM4 21h16" />,
  arrow: <path d="M5 19 19 5M10 5h9v9" />,
  rect: <rect x="4" y="6" width="16" height="12" rx="1" />,
  text: <path d="M5 6V4h14v2M12 4v16M9 20h6" />,
  step: (
    <>
      <circle cx="12" cy="12" r="8" />
      <path d="M11 9l2-1v8" />
    </>
  ),
  blur: <path d="M4 4h5v5H4zM15 4h5v5h-5zM9.5 9.5h5v5h-5zM4 15h5v5H4zM15 15h5v5h-5z" />,
  crop: <path d="M7 3v14h14M3 7h14v14" />,
  undo: <path d="M9 7 4 12l5 5M4 12h11a5 5 0 0 1 0 10h-2" />,
  redo: <path d="m15 7 5 5-5 5M20 12H9a5 5 0 0 0 0 10h2" />,
  rotl: <path d="M4 4v6h6M4.5 9.5A8 8 0 1 1 6 18" />,
  rotr: <path d="M20 4v6h-6M19.5 9.5A8 8 0 1 0 18 18" />,
  layer: (
    <>
      <rect x="3" y="7" width="12" height="12" rx="1.5" />
      <path d="M9 3h12v12M9 13l2.5-2.5L15 14" />
    </>
  ),
};

export function ToolIcon({ tool }: { tool: IconName }) {
  return (
    <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={1.6} strokeLinecap="round" strokeLinejoin="round" aria-hidden>
      {PATHS[tool]}
    </svg>
  );
}
