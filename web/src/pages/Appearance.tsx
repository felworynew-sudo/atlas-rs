import { useState } from 'react';
import { Header } from '../ui';

type Mode = 'system' | 'light' | 'dark';
const KEY = 'atlas.theme';

/** Применяет сохранённую тему (вызывается при старте и при выборе). «Системная» — без принудительного значения. */
export function applyTheme(mode: Mode): void {
  if (mode === 'system') document.documentElement.removeAttribute('data-theme');
  else document.documentElement.setAttribute('data-theme', mode);
}

export function savedTheme(): Mode {
  try {
    const v = localStorage.getItem(KEY);
    return v === 'light' || v === 'dark' ? v : 'system';
  } catch {
    return 'system';
  }
}

/** Внешний вид: системная, светлая или тёмная тема; выбор хранится на устройстве. */
export default function Appearance() {
  const [mode, setMode] = useState<Mode>(savedTheme());
  const pick = (m: Mode) => {
    setMode(m);
    try {
      if (m === 'system') localStorage.removeItem(KEY);
      else localStorage.setItem(KEY, m);
    } catch {
      /* без хранилища — до перезагрузки */
    }
    applyTheme(m);
  };
  return (
    <>
      <Header title="Внешний вид" />
      <main className="page">
        <span className="label">Тема</span>
        <div className="chips">
          {([['system', 'Системная'], ['light', 'Светлая'], ['dark', 'Тёмная']] as [Mode, string][]).map(([m, l]) => (
            <button key={m} className={`chip${mode === m ? ' active' : ''}`} onClick={() => pick(m)}>{l}</button>
          ))}
        </div>
        <div className="sub">Выбор сохраняется на этом устройстве.</div>
      </main>
    </>
  );
}
