import { useEffect, useState, type ReactNode } from 'react';
import { useNavigate } from 'react-router-dom';
import { Icon } from './Icon';

/** Шапка страницы: «назад», заголовок, действие справа (высота 64px). */
export function Header({ title, right, back = true }: { title: string; right?: ReactNode; back?: boolean }) {
  const nav = useNavigate();
  return (
    <header className="header">
      {back && (
        <button className="icon-btn small" aria-label="Назад" onClick={() => nav(-1)}>
          <Icon name="back" />
        </button>
      )}
      <h1>{title}</h1>
      {right}
    </header>
  );
}

export function Spinner() {
  return <div className="spinner" role="status" aria-label="Загрузка" />;
}

export function Modal({ onClose, children, className = '' }: { onClose: () => void; children: ReactNode; className?: string }) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === 'Escape' && onClose();
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [onClose]);
  return (
    <div className="modal-back" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className={`modal ${className}`} role="dialog" aria-modal="true">
        <button className="close" aria-label="Закрыть" onClick={onClose}>
          <Icon name="close" size={16} />
        </button>
        {children}
      </div>
    </div>
  );
}

export function Chips<T extends string>({ options, value, onChange }: { options: { id: T; label: string }[]; value: T; onChange: (id: T) => void }) {
  return (
    <div className="chips">
      {options.map((o) => (
        <button key={o.id} className={`chip${o.id === value ? ' active' : ''}`} onClick={() => onChange(o.id)}>
          {o.label}
        </button>
      ))}
    </div>
  );
}

/** Чипы филиалов; `all` добавляет пункт «Все». */
export function StoreChips({ stores, value, onChange, all = false }: { stores: { id: string; name: string }[]; value: string; onChange: (id: string) => void; all?: boolean }) {
  return (
    <div className="chips">
      {all && <button className={`chip${value === 'all' ? ' active' : ''}`} onClick={() => onChange('all')}>Все</button>}
      {stores.map((st) => (
        <button key={st.id} className={`chip${value === st.id ? ' active' : ''}`} onClick={() => onChange(st.id)}>{st.name}</button>
      ))}
    </div>
  );
}

/** «Фильтр по дате»: диапазон из двух дат. */
export function DateRange({ from, to, onChange }: { from: string; to: string; onChange: (from: string, to: string) => void }) {
  return (
    <>
      <span className="label">Фильтр по дате</span>
      <div className="row" style={{ marginBottom: 16 }}>
        <input type="date" aria-label="С" value={from} max={to} onChange={(e) => onChange(e.target.value, to)} />
        <input type="date" aria-label="По" value={to} min={from} onChange={(e) => onChange(from, e.target.value)} />
      </div>
    </>
  );
}

/** Короткое сообщение внизу экрана («Успешно»). */
export function useToast(): [ReactNode, (text: string) => void] {
  const [text, setText] = useState<string | null>(null);
  useEffect(() => {
    if (!text) return;
    const t = setTimeout(() => setText(null), 2500);
    return () => clearTimeout(t);
  }, [text]);
  return [text ? <div className="toast" role="status">{text}</div> : null, setText];
}

/** Запрос данных с индикатором загрузки и ошибкой. */
export function useLoad<T>(load: () => Promise<T>, deps: unknown[]): { data: T | null; error: string | null; loading: boolean; reload: () => void } {
  const [data, setData] = useState<T | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [tick, setTick] = useState(0);
  useEffect(() => {
    let alive = true;
    setLoading(true);
    load()
      .then((d) => {
        if (alive) {
          setData(d);
          setError(null);
        }
      })
      .catch((e: unknown) => alive && setError(e instanceof Error ? e.message : 'Ошибка'))
      .finally(() => alive && setLoading(false));
    return () => {
      alive = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [...deps, tick]);
  return { data, error, loading, reload: () => setTick((t) => t + 1) };
}

export function useMedia(query: string): boolean {
  const [m, setM] = useState(() => window.matchMedia(query).matches);
  useEffect(() => {
    const mq = window.matchMedia(query);
    const on = () => setM(mq.matches);
    mq.addEventListener('change', on);
    return () => mq.removeEventListener('change', on);
  }, [query]);
  return m;
}

/** Копирует текст в буфер; если браузер не даёт (например, внутри рамки) — через скрытое поле. */
export async function copyText(text: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    /* нет доступа к буферу — пробуем запасной путь */
  }
  try {
    const field = document.createElement('textarea');
    field.value = text;
    field.setAttribute('readonly', '');
    field.style.position = 'fixed';
    field.style.opacity = '0';
    document.body.appendChild(field);
    field.select();
    const ok = document.execCommand('copy');
    field.remove();
    return ok;
  } catch {
    return false;
  }
}
