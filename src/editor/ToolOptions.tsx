// Colour, stroke width and font size: the same controls in the editor and
// in the screen capture window.
import type { CSSProperties } from "react";
import { COLORS, FONT_SIZES, WIDTH_MAX, WIDTH_MIN } from "./draw";
import { nextFont, STROKE_TOOLS } from "./marks";

interface Props {
  tool: string;
  color: string;
  onColor: (c: string) => void;
  width: number;
  onWidth: (w: number) => void;
  font: number;
  onFont: (f: number) => void;
}

export function ToolOptions({ tool, color, onColor, width, onWidth, font, onFont }: Props) {
  const strokeTool = STROKE_TOOLS.includes(tool);
  return (
    <>
      <div className="ed-group">
        {COLORS.map((c, i) => (
          <button
            key={c}
            className={`ed-color${color === c ? " active" : ""}`}
            style={{ "--c": c } as CSSProperties}
            title={`Цвет (${i + 1})`}
            onClick={() => onColor(c)}
          />
        ))}
      </div>
      <span className="ed-sep" />
      {tool === "text" ? (
        <div className="ed-group ed-font" title="Размер шрифта ([ и ])">
          <button className="ed-tool" onClick={() => onFont(nextFont(font, -1))} disabled={font <= FONT_SIZES[0]} title="Меньше ([)">
            <span className="ed-a small">A</span>
          </button>
          <select value={font} onChange={(e) => onFont(Number(e.target.value))}>
            {FONT_SIZES.map((f) => (
              <option key={f} value={f}>
                {f}
              </option>
            ))}
          </select>
          <button
            className="ed-tool"
            onClick={() => onFont(nextFont(font, 1))}
            disabled={font >= FONT_SIZES[FONT_SIZES.length - 1]}
            title="Больше (])"
          >
            <span className="ed-a">A</span>
          </button>
        </div>
      ) : (
        <label className={`ed-group ed-width${strokeTool ? "" : " off"}`} title="Толщина ([ и ])">
          <i style={{ width: Math.min(18, 2 + width * 0.6), height: Math.min(18, 2 + width * 0.6) }} />
          <input
            type="range"
            min={WIDTH_MIN}
            max={WIDTH_MAX}
            value={width}
            disabled={!strokeTool}
            onChange={(e) => onWidth(Number(e.target.value))}
          />
          <span className="ed-width-value">{width}</span>
        </label>
      )}
    </>
  );
}
