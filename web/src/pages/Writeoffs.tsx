import { useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { api, type Writeoff } from '../api';
import { endOfDay, fmtDateTime, startOfDay, todayLocal } from '../dates';
import { Icon } from '../Icon';
import { num, rub } from '../money';
import { useSession } from '../session';
import { DateRange, Header, Spinner, StoreChips, useLoad, useToast } from '../ui';

export default function Writeoffs() {
  const s = useSession();
  const nav = useNavigate();
  const [toast, say] = useToast();
  const [from, setFrom] = useState(todayLocal());
  const [to, setTo] = useState(todayLocal());
  const [store, setStore] = useState('all');
  const list = useLoad(() => api<Writeoff[]>('/writeoffs', { query: { from: startOfDay(from), to: endOfDay(to), storeId: store === 'all' ? undefined : store } }), [from, to, store]);

  async function remove(id: string) {
    if (!confirm('Удалить списание? Остаток вернётся на склад.')) return;
    try {
      await api(`/writeoffs/${id}`, { method: 'DELETE' });
      list.reload();
    } catch (e) {
      say(e instanceof Error ? e.message : 'Ошибка');
    }
  }

  return (
    <>
      <Header title="Списания" right={<button className="icon-btn" aria-label="Новое списание" onClick={() => nav('/writeoff/new')}><Icon name="plus" /></button>} />
      <main className="page">
        <DateRange from={from} to={to} onChange={(a, b) => { setFrom(a); setTo(b); }} />
        <StoreChips all stores={s.stores} value={store} onChange={setStore} />
        <button className="link" onClick={list.reload}>Обновить</button>
        {list.loading && <Spinner />}
        {!list.loading && (list.data ?? []).length === 0 && <div className="empty">Ничего не найдено</div>}
        {(list.data ?? []).map((w) => (
          <div key={w.id} className="card row">
            <div className="grow">
              <div className="title">{w.productName} × {num(w.qty)}</div>
              <div className="sub">{fmtDateTime(w.writeoffDate)} · {w.reasonName} · {s.stores.find((x) => x.id === w.storeId)?.name}</div>
            </div>
            <span className="price low">{rub(w.amount)}</span>
            {s.can('writeoff.delete') && <button className="icon-btn small" aria-label="Удалить списание" onClick={() => remove(w.id)}><Icon name="trash" size={18} /></button>}
          </div>
        ))}
      </main>
      {toast}
    </>
  );
}
