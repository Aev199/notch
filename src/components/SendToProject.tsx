// «В проект…»: picks one of Papka's projects and sends the item there
// with an optional caption. Papka shows it in the project card under
// «Входящие»; it need not be running. Format: Papka's docs/INBOX.md.
import { useEffect, useRef, useState } from "react";
import { api, errorText, type PapkaProject } from "../lib/api";
import { CloseIcon } from "./Icons";

interface Props {
  /** What is sent, for the heading: «снимок», «текст», «ссылку на файл». */
  what: string;
  /** Writes the entry; resolves with the project's name. */
  send: (project: string, caption: string) => Promise<string>;
  onDone: (message: string) => void;
  onClose: () => void;
}

/** Case- and ё-insensitive form for matching project names. */
const fold = (s: string) => s.toLocaleLowerCase("ru").replace(/ё/g, "е");

export function SendToProject({ what, send, onDone, onClose }: Props) {
  const [projects, setProjects] = useState<PapkaProject[] | null>(null);
  const [chosen, setChosen] = useState<string | null>(null);
  const [caption, setCaption] = useState("");
  const [filter, setFilter] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const captionField = useRef<HTMLInputElement>(null);

  useEffect(() => {
    api
      .papkaProjects()
      .then((list) => {
        setProjects(list.projects);
        setChosen(list.last ?? list.projects[0]?.id ?? null);
        captionField.current?.focus();
      })
      .catch((e) => setError(errorText(e)));
  }, []);

  const shown = (projects ?? []).filter((p) => !filter || fold(p.name).includes(fold(filter.trim())));
  const target = projects?.find((p) => p.id === chosen);

  const submit = async () => {
    if (!chosen || busy) return;
    setBusy(true);
    setError(null);
    try {
      const name = await send(chosen, caption.trim());
      onDone(`Отправлено в «${name}»`);
    } catch (e) {
      setError(errorText(e));
      setBusy(false);
    }
  };

  // The caption field took the keyboard (in the island that is input mode):
  // closing the panel, by Esc, ✕ or a send, gives it back.
  useEffect(() => () => void api.endInput(), []);

  const keys = (e: React.KeyboardEvent) => {
    if (e.key === "Escape") {
      // Only this panel closes, not the island or the capture.
      e.stopPropagation();
      e.preventDefault();
      onClose();
    } else if (e.key === "Enter") {
      e.stopPropagation();
      e.preventDefault();
      void submit();
    }
  };

  return (
    <div className="papka-send" onPointerDown={(e) => e.stopPropagation()} onKeyDown={keys}>
      <div className="papka-head">
        <span>В проект Papka: {what}</span>
        <button className="icon-btn small" title="Закрыть (Esc)" onClick={onClose}>
          <CloseIcon width={13} height={13} />
        </button>
      </div>
      {projects === null && !error && <p className="dim">Читаю список проектов…</p>}
      {projects && projects.length === 0 && <p className="dim">В Papka пока нет проектов: создайте проект в её боковой панели.</p>}
      {projects && projects.length > 6 && (
        <input className="papka-filter" value={filter} placeholder="Найти проект" spellCheck={false} onChange={(e) => setFilter(e.target.value)} />
      )}
      {projects && projects.length > 0 && (
        <div className="papka-list">
          {shown.map((p) => (
            <button
              key={p.id}
              className={`papka-project${p.id === chosen ? " active" : ""}`}
              title={`Входящие: ${p.inbox}`}
              onClick={() => setChosen(p.id)}
              onDoubleClick={() => {
                setChosen(p.id);
                void submit();
              }}
            >
              {p.name}
            </button>
          ))}
          {shown.length === 0 && <span className="dim">ничего не найдено</span>}
        </div>
      )}
      <input
        ref={captionField}
        className="papka-caption"
        value={caption}
        placeholder="Подпись: зачем сохранено (можно пусто)"
        spellCheck={false}
        maxLength={500}
        onChange={(e) => setCaption(e.target.value)}
      />
      <div className="papka-foot">
        <span className={error ? "error" : "dim"} title={target?.inbox}>
          {error ?? (target ? `в «Входящие» проекта «${target.name}»` : "")}
        </span>
        <button className="btn primary" disabled={!chosen || busy} onClick={() => void submit()}>
          {busy ? "Отправляю…" : "Отправить"}
        </button>
      </div>
    </div>
  );
}
