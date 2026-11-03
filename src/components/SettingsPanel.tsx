import { useEffect, useState } from "react";
import { api, errorText, type Provider, type Settings } from "../lib/api";

interface Props {
  settings: Settings;
  autostart: boolean;
  dataDir: string;
  onSaved: (s: Settings) => void;
  onAutostart: (enabled: boolean) => void;
}

const PROVIDERS: { id: Provider; name: string }[] = [
  { id: "offline", name: "Офлайн (встроенный, без сети)" },
  { id: "google", name: "Google (нужен интернет)" },
  { id: "deepl", name: "DeepL API" },
  { id: "llm", name: "Локальная модель" },
];

export function SettingsPanel({ settings, autostart, dataDir, onSaved, onAutostart }: Props) {
  const [draft, setDraft] = useState(settings);
  const [status, setStatus] = useState<{ ok: boolean; text: string } | null>(null);

  useEffect(() => setDraft(settings), [settings]);

  const set = <K extends keyof Settings>(key: K, value: Settings[K]) => {
    setDraft((d) => ({ ...d, [key]: value }));
    setStatus(null);
  };

  const save = async () => {
    try {
      const saved = await api.saveSettings(draft);
      onSaved(saved);
      setStatus({ ok: true, text: "Сохранено" });
    } catch (e) {
      setStatus({ ok: false, text: errorText(e) });
    }
  };

  const toggleAutostart = async (enabled: boolean) => {
    try {
      onAutostart(await api.setAutostart(enabled));
    } catch (e) {
      setStatus({ ok: false, text: errorText(e) });
    }
  };

  const changed = JSON.stringify(draft) !== JSON.stringify(settings);

  return (
    <div className="settings">
      <section>
        <h3>Переводчик</h3>
        <label className="row">
          <span>Сервис</span>
          <select value={draft.provider} onChange={(e) => set("provider", e.target.value as Provider)}>
            {PROVIDERS.map((p) => (
              <option key={p.id} value={p.id}>
                {p.name}
              </option>
            ))}
          </select>
        </label>
        {draft.provider === "deepl" && (
          <label className="row">
            <span>Ключ DeepL</span>
            <input type="password" value={draft.deeplKey} placeholder="xxxx:fx" onChange={(e) => set("deeplKey", e.target.value)} />
          </label>
        )}
        {draft.provider === "llm" && (
          <>
            <label className="row">
              <span>Адрес</span>
              <input value={draft.llmUrl} onChange={(e) => set("llmUrl", e.target.value)} />
            </label>
            <label className="row">
              <span>Модель</span>
              <input value={draft.llmModel} onChange={(e) => set("llmModel", e.target.value)} />
            </label>
            <label className="row">
              <span>Ключ (если нужен)</span>
              <input type="password" value={draft.llmKey} onChange={(e) => set("llmKey", e.target.value)} />
            </label>
          </>
        )}
        <label className="row">
          <span>Горячая клавиша</span>
          <input value={draft.hotkey} placeholder="Ctrl+Alt+T" onChange={(e) => set("hotkey", e.target.value)} />
        </label>
        <label className="check">
          <input type="checkbox" checked={draft.doubleCopyTranslate} onChange={(e) => set("doubleCopyTranslate", e.target.checked)} />
          <span>Переводить по двойному Ctrl+C</span>
        </label>
      </section>

      <section>
        <h3>Буфер и полка</h3>
        <label className="check">
          <input type="checkbox" checked={draft.captureImages} onChange={(e) => set("captureImages", e.target.checked)} />
          <span>Складывать скриншоты на полку</span>
        </label>
        <label className="row">
          <span>Скриншотов на полке</span>
          <input type="number" min={1} max={100} value={draft.shelfLimit} onChange={(e) => set("shelfLimit", Number(e.target.value))} />
        </label>
        <label className="row">
          <span>Текстов в истории</span>
          <input type="number" min={5} max={100} value={draft.textLimit} onChange={(e) => set("textLimit", Number(e.target.value))} />
        </label>
        <label className="check">
          <input type="checkbox" checked={draft.persistTextHistory} onChange={(e) => set("persistTextHistory", e.target.checked)} />
          <span>Хранить историю текста между запусками (в файле рядом с программой)</span>
        </label>
      </section>

      <section>
        <h3>Поведение</h3>
        <label className="check">
          <input type="checkbox" checked={draft.idleThin} onChange={(e) => set("idleThin", e.target.checked)} />
          <span>Сворачивать в тонкую полосу при бездействии</span>
        </label>
        {draft.idleThin && (
          <label className="row">
            <span>Через, секунд</span>
            <input type="number" min={1} max={600} value={draft.idleSeconds} onChange={(e) => set("idleSeconds", Number(e.target.value))} />
          </label>
        )}
        <label className="check">
          <input type="checkbox" checked={draft.hideInFullscreen} onChange={(e) => set("hideInFullscreen", e.target.checked)} />
          <span>Прятать совсем в эксклюзивном полноэкранном режиме игр и презентаций</span>
        </label>
        <label className="check">
          <input type="checkbox" checked={autostart} onChange={(e) => void toggleAutostart(e.target.checked)} />
          <span>Запускать вместе с Windows</span>
        </label>
      </section>

      <div className="settings-foot">
        <button className="btn primary" disabled={!changed} onClick={() => void save()}>
          Сохранить
        </button>
        {status && <span className={status.ok ? "ok" : "error"}>{status.text}</span>}
        <span className="push" />
        <button className="btn" title={dataDir} onClick={() => void api.openDataFolder()}>
          Папка
        </button>
        <button className="btn danger" onClick={() => void api.quit()}>
          Выход
        </button>
      </div>
    </div>
  );
}
