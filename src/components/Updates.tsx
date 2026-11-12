import { useEffect, useState } from "react";
import { api, errorText, on, type UpdateInfo, type UpdateStatus } from "../lib/api";

const mb = (n: number) => `${(n / 1048576).toFixed(1)} МБ`;

function describe(s: UpdateStatus): string | null {
  switch (s.state) {
    case "checking":
      return "Проверяю…";
    case "upToDate":
      return "Установлена последняя версия";
    case "available":
      return s.from_file ? `В «Загрузках» найдена версия ${s.update.version}` : `Доступна версия ${s.update.version}`;
    case "downloading":
      return `Скачиваю… ${mb(s.done)}${s.total ? ` из ${mb(s.total)}` : ""}`;
    case "ready":
      return `Версия ${s.version} установлена — нужен перезапуск`;
    case "offline":
      return "Нет доступа к GitHub — скачайте через браузер";
    case "watching":
      return "Жду файл в «Загрузках»… (20 минут)";
    case "failed":
      return `Не получилось: ${s.error}`;
    default:
      return null;
  }
}

/** Settings section; also used to show the badge state. */
export function Updates({ onError }: { onError: (m: string) => void }) {
  const [info, setInfo] = useState<UpdateInfo | null>(null);

  useEffect(() => {
    void api.updateInfo().then(setInfo);
    const off = on<UpdateStatus>("update", (status) => setInfo((i) => (i ? { ...i, status } : i)));
    return () => void off.then((f) => f());
  }, []);

  if (!info) return null;
  const s = info.status;
  const run = (p: Promise<unknown>) => void p.catch((e) => onError(errorText(e)));
  const line = describe(s);
  const notes = s.state === "available" ? s.update.notes : [];

  return (
    <section>
      <h3>Обновления</h3>
      <div className="update-row">
        <span>Версия {info.version}</span>
        {line && <span className={s.state === "failed" ? "error" : s.state === "available" || s.state === "ready" ? "ok" : "dim"}>{line}</span>}
      </div>
      {notes.length > 0 && (
        <ul className="update-notes">
          {notes.slice(0, 6).map((n) => (
            <li key={n}>{n}</li>
          ))}
        </ul>
      )}
      <div className="update-actions">
        {s.state === "available" && (
          <button className="btn primary" onClick={() => run(api.updateInstall())}>
            Установить {s.update.version}
          </button>
        )}
        {s.state === "ready" && (
          <button className="btn primary" onClick={() => run(api.updateRestart())}>
            Перезапустить
          </button>
        )}
        {(s.state === "offline" || s.state === "failed") && (
          <button className="btn primary" onClick={() => run(api.updateBrowser())} title={info.browserUrl}>
            Скачать в браузере
          </button>
        )}
        {s.state !== "checking" && s.state !== "downloading" && s.state !== "ready" && (
          <button className="btn" onClick={() => run(api.updateCheck())}>
            Проверить
          </button>
        )}
        {s.state !== "offline" && s.state !== "failed" && s.state !== "ready" && s.state !== "available" && (
          <button className="btn" onClick={() => run(api.updateBrowser())} title={info.browserUrl}>
            Скачать в браузере
          </button>
        )}
        {info.hasPrevious && s.state !== "ready" && (
          <button className="btn" onClick={() => run(api.updateRollback())} title="Поменять местами notch.exe и notch.old.exe и перезапустить">
            Вернуть предыдущую
          </button>
        )}
      </div>
    </section>
  );
}

/** True when the gear should carry a dot. */
export function updatePending(s: UpdateStatus | null): boolean {
  return !!s && (s.state === "available" || s.state === "ready");
}
