import { useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { api, type Transfer } from '../api';
import { daysAgoLocal, endOfDay, fmtDateTime, startOfDay, todayLocal } from '../dates';
import { Icon } from '../Icon';
import { num, rub } from '../money';
import { useSession } from '../session';
import { DateRange, Header, Spinner, useLoad, useToast } from '../ui';

/** Перемещения товаров между филиалами. */
export default function Transfers() {
  const s = useSession();
  const nav = useNavigate();
  const [toast, say] = useToast();
  const [from, setFrom] = useState(daysAgoLocal(30));
  const [to, setTo] = useState(todayLocal());
  const list = useLoad(() => api<Transfer[]>('/transfers', { query: { from: startOfDay(from), to: endOfDay(to) } }), [from, to]);
  const [openId, setOpenId] = useState<string | null>(null);
  const store = (id: string) => s.stores.find((x) => x.id === id)?.name ?? '—';

  async function remove(id: string) {
    if (!confirm('Удалить перемещение? Остатки вернутся.')) return;
    try {
      await api(`/transfers/${id}`, { method: 'DELETE' });
      list.reload();
    } catch (e) {
      say(e instanceof Error ? e.message : 'Ошибка');
    }
  }

  return (
    <>
      <Header title="Перемещения" right={<button className="icon-btn" aria-label="Новое перемещение" onClick={() => nav('/transfer/new')}><Icon name="plus" /></button>} />
      <main className="page">
        <DateRange from={from} to={to} onChange={(a, b) => { setFrom(a); setTo(b); }} />
        {list.loading && <Spinner />}
        {list.error && <div className="error">{list.error}</div>}
        {!list.loading && (list.data ?? []).length === 0 && <div className="empty">Ничего не найдено</div>}
        {(list.data ?? []).map((t) => (
          <div key={t.id} className="card">
            <div className="row click" onClick={() => setOpenId(openId === t.id ? null : t.id)}>
              <div className="grow">
                <div className="title">{store(t.fromStoreId)} → {store(t.toStoreId)}</div>
                <div className="sub">{fmtDateTime(t.transferDate)}{t.comment ? ` · ${t.comment}` : ''}</div>
              </div>
              <span className="price">{rub(t.amount)}</span>
            </div>
            {openId === t.id && (
              <div style={{ marginTop: 8 }}>
                {t.items.map((i) => <div key={i.productId} className="total-row"><span>{i.name}</span><span>{num(i.qty)}</span></div>)}
                {s.can('transfer.delete') && <button className="link" onClick={() => remove(t.id)}>Удалить</button>}
              </div>
            )}
          </div>
        ))}
      </main>
      {toast}
    </>
  );
}
