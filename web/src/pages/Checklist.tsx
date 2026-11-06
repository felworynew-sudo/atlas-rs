import { useEffect, useState } from 'react';
import { api, type ChecklistItem } from '../api';
import { fmtDateTime, todayLocal } from '../dates';
import { Icon } from '../Icon';
import { useSession } from '../session';
import { Header, Spinner, StoreChips, useLoad, useToast } from '../ui';

interface Draft { id?: string; title: string; time: string; kind: 'daily' | 'once' | 'section' }
const KINDS = [['daily', 'Ежедневная задача'], ['once', 'Разовая задача'], ['section', 'Раздел']] as const;

/** Чеклист дня: отметки с временем и сотрудником (сбрасываются каждый день), режим редактирования списка. */
export default function Checklist() {
  const s = useSession();
  const [toast, say] = useToast();
  const [store, setStore] = useState(s.storeId ?? '');
  const [date, setDate] = useState(todayLocal());
  const list = useLoad(() => api<ChecklistItem[]>('/checklist', { query: { storeId: store, date } }), [store, date]);
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState<Draft[]>([]);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    setDraft((list.data ?? []).map((i) => ({ id: i.id, title: i.title, time: i.time, kind: i.kind })));
  }, [list.data]);

  async function toggle(id: string) {
    try {
      await api(`/checklist/items/${id}/toggle`, { body: { date } });
      list.reload();
    } catch (e) {
      say(e instanceof Error ? e.message : 'Ошибка');
    }
  }

  const move = (i: number, d: number) => setDraft((cur) => {
    const n = [...cur];
    const j = i + d;
    if (j < 0 || j >= n.length) return cur;
    [n[i], n[j]] = [n[j], n[i]];
    return n;
  });
  const patch = (i: number, p: Partial<Draft>) => setDraft((cur) => cur.map((x, k) => (k === i ? { ...x, ...p } : x)));

  async function save() {
    setError(null);
    try {
      await api('/checklist/items', { method: 'PUT', body: { storeId: store, items: draft.map((x) => ({ id: x.id, title: x.title, time: x.time, kind: x.kind })) } });
      setEditing(false);
      list.reload();
      say('Успешно');
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Ошибка');
    }
  }

  return (
    <>
      <Header title="Чеклист" right={s.can('checklist.edit') ? <button className="icon-btn" aria-label={editing ? 'Закончить редактирование' : 'Редактировать'} onClick={() => setEditing(!editing)}><Icon name={editing ? 'close' : 'edit'} /></button> : undefined} />
      <main className="page">
        <label className="field"><span>Фильтр по дате</span><input type="date" value={date} onChange={(e) => setDate(e.target.value)} /></label>
        <StoreChips stores={s.stores} value={store} onChange={setStore} />
        {list.loading && <Spinner />}

        {!editing && (
          <>
            {!list.loading && (list.data ?? []).length === 0 && <div className="empty">Задач пока нет</div>}
            {(list.data ?? []).map((i) =>
              i.kind === 'section' ? (
                <div key={i.id} className="day" style={{ fontSize: 12, color: 'var(--text)' }}>{i.title}</div>
              ) : (
                <label key={i.id} className="card row" style={{ cursor: 'pointer' }}>
                  <input type="checkbox" checked={i.done} onChange={() => toggle(i.id)} style={{ width: 22, height: 22 }} />
                  <div className="grow">
                    <div className="title" style={i.done ? { textDecoration: 'line-through', opacity: 0.7 } : undefined}>{i.title}{i.time ? ` · ${i.time}` : ''}</div>
                    {i.done && i.doneAt && <div className="sub">Выполнено {fmtDateTime(i.doneAt)}, {i.doneBy}</div>}
                  </div>
                </label>
              ),
            )}
          </>
        )}

        {editing && (
          <>
            {draft.map((x, i) => (
              <div key={x.id ?? `n${i}`} className="card">
                <div className="row">
                  <input type="text" aria-label="Название" value={x.title} placeholder="Название" onChange={(e) => patch(i, { title: e.target.value })} />
                  <input type="text" aria-label="Время" value={x.time} placeholder="ЧЧ:ММ" style={{ maxWidth: 100 }} onChange={(e) => patch(i, { time: e.target.value })} />
                </div>
                <div className="row" style={{ marginTop: 8 }}>
                  <select aria-label="Тип" value={x.kind} onChange={(e) => patch(i, { kind: e.target.value as Draft['kind'] })}>
                    {KINDS.map(([v, l]) => <option key={v} value={v}>{l}</option>)}
                  </select>
                  <button className="qty-btn" aria-label="Вверх" onClick={() => move(i, -1)}>↑</button>
                  <button className="qty-btn" aria-label="Вниз" onClick={() => move(i, 1)}>↓</button>
                  <button className="icon-btn small" aria-label="Удалить" onClick={() => setDraft((cur) => cur.filter((_, k) => k !== i))}><Icon name="trash" size={18} /></button>
                </div>
              </div>
            ))}
            <button className="btn light" style={{ margin: '8px 0 16px' }} onClick={() => setDraft([...draft, { title: '', time: '', kind: 'daily' }])}>+ Добавить задачу</button>
            {error && <div className="error" role="alert">{error}</div>}
            <button className="btn" disabled={draft.some((x) => !x.title.trim())} onClick={save}>Сохранить</button>
          </>
        )}
      </main>
      {toast}
    </>
  );
}
