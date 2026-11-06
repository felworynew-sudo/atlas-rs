import { useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { api, type Manufacture } from '../api';
import { daysAgoLocal, endOfDay, fmtDateTime, startOfDay, todayLocal } from '../dates';
import { Icon } from '../Icon';
import { num, rub } from '../money';
import { useSession } from '../session';
import { DateRange, Header, Spinner, StoreChips, useLoad, useToast } from '../ui';

/** Производства: выпуск готового товара из ингредиентов. */
export default function Manufactures() {
  const s = useSession();
  const nav = useNavigate();
  const [toast, say] = useToast();
  const [store, setStore] = useState('all');
  const [from, setFrom] = useState(daysAgoLocal(30));
  const [to, setTo] = useState(todayLocal());
  const list = useLoad(() => api<Manufacture[]>('/manufactures', { query: { storeId: store === 'all' ? undefined : store, from: startOfDay(from), to: endOfDay(to) } }), [store, from, to]);
  const [openId, setOpenId] = useState<string | null>(null);

  async function remove(id: string) {
    if (!confirm('Удалить производство? Остатки вернутся.')) return;
    try {
      await api(`/manufactures/${id}`, { method: 'DELETE' });
      list.reload();
    } catch (e) {
      say(e instanceof Error ? e.message : 'Ошибка');
    }
  }

  return (
    <>
      <Header title="Производство" right={<button className="icon-btn" aria-label="Новое производство" onClick={() => nav('/manufacture/new')}><Icon name="plus" /></button>} />
      <main className="page">
        <DateRange from={from} to={to} onChange={(a, b) => { setFrom(a); setTo(b); }} />
        <StoreChips all stores={s.stores} value={store} onChange={setStore} />
        {list.loading && <Spinner />}
        {list.error && <div className="error">{list.error}</div>}
        {!list.loading && (list.data ?? []).length === 0 && <div className="empty">Ничего не найдено</div>}
        {(list.data ?? []).map((m) => (
          <div key={m.id} className="card">
            <div className="row click" onClick={() => setOpenId(openId === m.id ? null : m.id)}>
              <div className="grow">
                <div className="title">{m.productName} × {num(m.qty)}</div>
                <div className="sub">{fmtDateTime(m.manufactureDate)}{m.batch ? ` · партия ${m.batch}` : ''}</div>
              </div>
              <span className="price">{rub(m.costTotal)}</span>
            </div>
            {openId === m.id && (
              <div style={{ marginTop: 8 }}>
                {m.ingredients.map((i) => <div key={i.productId} className="total-row"><span>{i.name}</span><span>{num(i.qty)}</span></div>)}
                {s.can('manufacture.delete') && <button className="link" onClick={() => remove(m.id)}>Удалить</button>}
              </div>
            )}
          </div>
        ))}
      </main>
      {toast}
    </>
  );
}
