import { useEffect, useRef, useState, type PointerEvent as ReactPointerEvent } from "react";
import { startDrag } from "@crabnebula/tauri-plugin-drag";
import { api, errorText, sizeText, timeAgo, type FileEntry } from "../lib/api";
import { CloseIcon, FolderIcon, ProjectIcon } from "./Icons";
import { SendToProject } from "./SendToProject";

const DRAG_THRESHOLD = 5;

interface Props {
  entries: FileEntry[];
  /** Picture shown under the cursor while files are dragged out. */
  icon: string;
  onError: (message: string) => void;
  /** A short message that something was done. */
  onNotice: (message: string) => void;
}

/** "PDF" for the badge; folders and names without an extension get none. */
function extension(entry: FileEntry): string {
  if (entry.isDir) return "";
  const dot = entry.name.lastIndexOf(".");
  return dot > 0 ? entry.name.slice(dot + 1, dot + 5).toUpperCase() : "";
}

/** Folder of a path, for the second line. */
function folder(path: string): string {
  const cut = Math.max(path.lastIndexOf("\\"), path.lastIndexOf("/"));
  return cut > 0 ? path.slice(0, cut) : path;
}

export function Files({ entries, icon, onError, onNotice }: Props) {
  // The file being sent to a Papka project (as a link).
  const [sending, setSending] = useState<FileEntry | null>(null);
  const press = useRef<{ entry: FileEntry; x: number; y: number } | null>(null);

  // Files may have been moved or deleted since the tab was last looked at.
  useEffect(() => void api.filesRefresh(), []);

  /** Hands the files over to Windows: they leave the notch. */
  const dragOut = async (paths: string[]) => {
    await api.setDragging(true);
    try {
      await startDrag({ item: paths, icon }, () => void api.setDragging(false));
    } catch (e) {
      onError(errorText(e));
    } finally {
      void api.setDragging(false);
    }
  };

  const onPointerDown = (e: ReactPointerEvent, entry: FileEntry) => {
    if (e.button !== 0 || (e.target as HTMLElement).closest("button")) return;
    press.current = { entry, x: e.clientX, y: e.clientY };
  };

  const onPointerMove = (e: ReactPointerEvent) => {
    const p = press.current;
    if (!p || Math.hypot(e.clientX - p.x, e.clientY - p.y) < DRAG_THRESHOLD) return;
    press.current = null;
    if (p.entry.missing) {
      onError("файла больше нет на этом месте");
      return;
    }
    void dragOut([p.entry.path]);
  };

  const onPointerUp = (entry: FileEntry) => {
    const p = press.current;
    press.current = null;
    // A plain click opens the file.
    if (p?.entry.id === entry.id) void api.filesOpen(entry.id).catch((e) => onError(errorText(e)));
  };

  if (entries.length === 0) {
    return (
      <div className="empty">
        <p>Сюда можно положить файлы</p>
        <span>
          Перетащите файл или папку на чёлку и подержите секунду — она раскроется. Отсюда их можно вытащить в любое окно.
          Сами файлы остаются на своих местах.
        </span>
      </div>
    );
  }

  const present = entries.filter((e) => !e.missing);

  return (
    <div className="files-wrap" onPointerMove={onPointerMove} onPointerLeave={() => (press.current = null)}>
      {present.length > 1 && (
        <div className="files-bar">
          <span className="dim">{present.length} шт.</span>
          <span className="push" />
          <button
            className="btn grab"
            title="Вытащить все файлы разом в окно или папку"
            onPointerDown={(e) => {
              if (e.button === 0) void dragOut(present.map((f) => f.path));
            }}
          >
            Тащить все
          </button>
        </div>
      )}
      <div className="files">
        {entries.map((entry) => (
          <div
            key={entry.id}
            className={`file${entry.missing ? " missing" : ""}`}
            role="button"
            tabIndex={-1}
            title={`${entry.path}\nКлик — открыть, потянуть — вытащить в другое окно`}
            onPointerDown={(e) => onPointerDown(e, entry)}
            onPointerUp={() => onPointerUp(entry)}
          >
            <span className={`file-badge${entry.isDir ? " dir" : ""}`}>{entry.isDir ? <FolderIcon width={14} height={14} /> : extension(entry)}</span>
            <span className="file-main">
              <span className="file-name">{entry.name}</span>
              <span className="file-path">{entry.missing ? "нет на месте: перемещён или удалён" : folder(entry.path)}</span>
            </span>
            <span className="clip-meta">{entry.isDir || entry.missing ? timeAgo(entry.addedAt) : sizeText(entry.size)}</span>
            <span className="clip-actions">
              {!entry.missing && (
                <button
                  className="icon-btn small"
                  title="Показать в папке"
                  onClick={() => void api.filesReveal(entry.id).catch((e) => onError(errorText(e)))}
                >
                  <FolderIcon width={13} height={13} />
                </button>
              )}
              {!entry.missing && (
                <button className="icon-btn small" title="В проект Papka… (ссылка на файл, сам файл не копируется)" onClick={() => setSending(entry)}>
                  <ProjectIcon width={13} height={13} />
                </button>
              )}
              <button className="icon-btn small" title="Убрать с полки (файл останется на месте)" onClick={() => void api.filesRemove(entry.id)}>
                <CloseIcon width={13} height={13} />
              </button>
            </span>
          </div>
        ))}
      </div>
      {sending && (
        <SendToProject
          what={sending.isDir ? "ссылку на папку" : "ссылку на файл"}
          send={(project, caption) => api.papkaSendFile(sending.id, project, caption)}
          onDone={(message) => {
            setSending(null);
            onNotice(message);
          }}
          onClose={() => setSending(null)}
        />
      )}
    </div>
  );
}
