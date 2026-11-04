import { useEffect, useMemo, useRef, useState, type PointerEvent as ReactPointerEvent } from "react";
import { startDrag } from "@crabnebula/tauri-plugin-drag";
import { api, errorText, timeAgo, type ShelfItem } from "../lib/api";
import { BackIcon, CheckIcon, CopyIcon, TrashIcon, UngroupIcon } from "./Icons";

const DRAG_THRESHOLD = 5;
/** Drop target meaning "take it out of the group". */
const OUT = "__out";

type Tile =
  | { kind: "shot"; key: string; item: ShelfItem }
  | { kind: "group"; key: string; items: ShelfItem[] };

interface Drag {
  key: string;
  ids: string[];
  paths: string[];
  icon: string;
  thumb: string;
  count: number;
  x: number;
  y: number;
  over: string | null;
}

interface Props {
  items: ShelfItem[];
  onError: (message: string) => void;
}

/** Loose screenshots and groups in one list, newest first. */
function tilesOf(items: ShelfItem[]): Tile[] {
  const tiles: Tile[] = [];
  const groups = new Map<string, ShelfItem[]>();
  for (const item of items) {
    if (!item.group) {
      tiles.push({ kind: "shot", key: item.id, item });
      continue;
    }
    let members = groups.get(item.group);
    if (!members) {
      members = [];
      groups.set(item.group, members);
      tiles.push({ kind: "group", key: item.group, items: members });
    }
    members.push(item);
  }
  return tiles;
}

function dragOf(tile: Tile, x: number, y: number): Drag {
  const items = tile.kind === "shot" ? [tile.item] : tile.items;
  return {
    key: tile.key,
    ids: [tile.key],
    paths: items.map((i) => i.path),
    icon: items[0].thumb_path,
    thumb: items[0].thumb,
    count: items.length,
    x,
    y,
    over: null,
  };
}

export function Shelf({ items, onError }: Props) {
  const [openGroup, setOpenGroup] = useState<string | null>(null);
  const [drag, setDrag] = useState<Drag | null>(null);
  const [copied, setCopied] = useState<string | null>(null);
  const press = useRef<{ tile: Tile; x: number; y: number; pointer: number } | null>(null);
  const dragRef = useRef<Drag | null>(null);
  const root = useRef<HTMLDivElement>(null);

  const all = useMemo(() => tilesOf(items), [items]);
  const groupItems = openGroup ? items.filter((i) => i.group === openGroup) : [];
  // The group vanished (emptied, deleted): back to the shelf.
  const groupGone = openGroup !== null && groupItems.length === 0;
  useEffect(() => {
    if (groupGone) setOpenGroup(null);
  }, [groupGone]);
  const tiles: Tile[] = openGroup ? groupItems.map((item) => ({ kind: "shot", key: item.id, item })) : all;

  const setDragState = (d: Drag | null) => {
    dragRef.current = d;
    setDrag(d);
  };

  /** Hands the drag over to Windows: the files leave the notch. */
  const dragOut = async (d: Drag) => {
    setDragState(null);
    await api.setDragging(true);
    try {
      await startDrag({ item: d.paths, icon: d.icon }, () => void api.setDragging(false));
    } catch (e) {
      onError(errorText(e));
    } finally {
      void api.setDragging(false);
    }
  };

  const onPointerDown = (e: ReactPointerEvent, tile: Tile) => {
    if (e.button !== 0 || (e.target as HTMLElement).closest("button")) return;
    press.current = { tile, x: e.clientX, y: e.clientY, pointer: e.pointerId };
  };

  const onPointerMove = (e: ReactPointerEvent) => {
    const p = press.current;
    const d = dragRef.current;
    if (!d) {
      if (!p || Math.hypot(e.clientX - p.x, e.clientY - p.y) < DRAG_THRESHOLD) return;
      root.current?.setPointerCapture(p.pointer);
      setDragState(dragOf(p.tile, e.clientX, e.clientY));
      return;
    }
    // Left the island: it is a drag to another program.
    const island = root.current?.closest(".island")?.getBoundingClientRect();
    if (island && (e.clientX < island.left || e.clientX > island.right || e.clientY > island.bottom - 2)) {
      press.current = null;
      root.current?.releasePointerCapture(e.pointerId);
      void dragOut(d);
      return;
    }
    const under = document.elementFromPoint(e.clientX, e.clientY)?.closest<HTMLElement>("[data-drop]");
    const over = under && under.dataset.drop !== d.key ? under.dataset.drop! : null;
    setDragState({ ...d, x: e.clientX, y: e.clientY, over });
  };

  const onPointerUp = () => {
    const d = dragRef.current;
    const p = press.current;
    press.current = null;
    if (!d) {
      // A plain click opens a group, or the screenshot in the editor.
      if (p?.tile.kind === "group") setOpenGroup(p.tile.key);
      else if (p?.tile.kind === "shot") void api.openEditor(p.tile.key).catch((e) => onError(errorText(e)));
      return;
    }
    setDragState(null);
    if (!d.over) return;
    if (d.over === OUT) {
      void api.shelfUngroup(d.key).catch((err) => onError(errorText(err)));
    } else {
      void api.shelfGroup(d.ids, d.over).catch((err) => onError(errorText(err)));
    }
  };

  const copy = async (item: ShelfItem) => {
    try {
      await api.shelfCopy(item.id);
      setCopied(item.id);
      window.setTimeout(() => setCopied((c) => (c === item.id ? null : c)), 1200);
    } catch (e) {
      onError(errorText(e));
    }
  };

  if (items.length === 0) {
    return (
      <div className="empty">
        <p>Здесь будут скриншоты из буфера</p>
        <span>Win+Shift+S или PrtScr. Клик — редактор, текст и перевод; за край — в другое окно, на другой скриншот — в группу.</span>
      </div>
    );
  }

  return (
    <div
      ref={root}
      className={`shelf-wrap${drag ? " dragging" : ""}`}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
      onPointerCancel={() => {
        press.current = null;
        setDragState(null);
      }}
    >
      {openGroup && (
        <div className="group-bar">
          <button
            className={`btn back${drag?.over === OUT ? " drop-hot" : ""}`}
            data-drop={drag ? OUT : undefined}
            onClick={() => setOpenGroup(null)}
            title={drag ? "Отпустите здесь, чтобы вынуть из группы" : "Назад к полке"}
          >
            <BackIcon width={14} height={14} />
            {drag ? "Вынуть из группы" : "Полка"}
          </button>
          <span className="dim">
            {groupItems.length} шт. · {timeAgo(groupItems[0]?.created_at ?? Date.now())}
          </span>
          <span className="push" />
          <button
            className="btn grab"
            title="Вытащить всю группу в окно или папку"
            onPointerDown={(e) => {
              if (e.button !== 0) return;
              const tile: Tile = { kind: "group", key: openGroup, items: groupItems };
              press.current = { tile, x: e.clientX, y: e.clientY, pointer: e.pointerId };
            }}
          >
            Тащить все
          </button>
          <button className="btn danger" onClick={() => void api.shelfRemoveGroup(openGroup)}>
            Удалить группу
          </button>
        </div>
      )}
      <div className="shelf-grid" key={openGroup ?? "all"}>
        {tiles.map((tile) => {
          const first = tile.kind === "shot" ? tile.item : tile.items[0];
          const hot = drag?.over === tile.key;
          const lifted = drag?.key === tile.key;
          return (
            <figure
              key={tile.key}
              className={`tile${tile.kind === "group" ? " group" : ""}${hot ? " drop-hot" : ""}${lifted ? " lifted" : ""}`}
              data-drop={tile.key}
              title={
                tile.kind === "group"
                  ? `Группа из ${tile.items.length}: клик — открыть, за край — вынести все файлы`
                  : `${first.width}×${first.height}: клик — редактор, текст и перевод; за край — в другое окно, на скриншот — в группу`
              }
              onPointerDown={(e) => onPointerDown(e, tile)}
            >
              {tile.kind === "group" ? (
                <div className={`mosaic n${Math.min(tile.items.length, 4)}`}>
                  {tile.items.slice(0, 4).map((it) => (
                    <img key={it.id} src={it.thumb} alt="" draggable={false} />
                  ))}
                </div>
              ) : (
                <img className="thumb" src={first.thumb} alt="" draggable={false} />
              )}
              <figcaption>
                <span>{tile.kind === "group" ? `Группа · ${tile.items.length}` : timeAgo(first.created_at)}</span>
                {tile.kind === "shot" && (
                  <span className="dim">
                    {first.width}×{first.height}
                  </span>
                )}
              </figcaption>
              <div className="tile-actions">
                {tile.kind === "shot" ? (
                  <>
                    <button className="icon-btn small" title="Копировать" onClick={() => void copy(first)}>
                      {copied === first.id ? <CheckIcon width={13} height={13} /> : <CopyIcon width={13} height={13} />}
                    </button>
                    {openGroup && (
                      <button className="icon-btn small" title="Вынуть из группы" onClick={() => void api.shelfUngroup(first.id)}>
                        <UngroupIcon width={13} height={13} />
                      </button>
                    )}
                    <button
                      className="icon-btn small danger"
                      title="Удалить"
                      onClick={() => void api.shelfRemove(first.id).catch((e) => onError(errorText(e)))}
                    >
                      <TrashIcon width={13} height={13} />
                    </button>
                  </>
                ) : (
                  <button className="icon-btn small danger" title="Удалить группу" onClick={() => void api.shelfRemoveGroup(tile.key)}>
                    <TrashIcon width={13} height={13} />
                  </button>
                )}
              </div>
            </figure>
          );
        })}
      </div>
      {drag && (
        <div className="drag-ghost" style={{ left: drag.x, top: drag.y }}>
          <img src={drag.thumb} alt="" />
          <span>
            {drag.count > 1 ? `${drag.count} · ` : ""}
            {drag.over ? (drag.over === OUT ? "вынуть из группы" : "в группу") : "за край — в другое окно"}
          </span>
        </div>
      )}
    </div>
  );
}
