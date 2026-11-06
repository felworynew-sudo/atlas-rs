import { useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { api, type Supplier, type Supply } from '../api';
import { fmtDate, todayLocal } from '../dates';
import { Icon } from '../Icon';
import { rub } from '../money';
import { useSession } from '../session';
import { DateRange, Header, Spinner, StoreChips, useLoad } from '../ui';

export default function Supplies() {
  const s = useSession();
  const nav = useNavigate();
  const [from, setFrom] = useState(todayLocal());
  const [to, setTo] = useState(todayLocal());
  const [store, setStore] = useState('all');
  const list = useLoad(() => api<Supply[]>('/supplies', { query: { from, to, storeId: store === 'all' ? undefined : store } }), [from, to, store]);
  const sups = useLoad(() => api<Supplier[]>('/suppliers'), []);
  const supName = (id: string | null) => (sups.data ?? []).find((x) => x.id === id)?.name ?? 'Без поставщика';

  return (
    <>
      <Header title="Приемки" right={<button className="icon-btn" aria-label="Новая приёмка" onClick={() => nav('/supply/new')}><Icon name="plus" /></button>} />
      <main className="page">
        <DateRange from={from} to={to} onChange={(a, b) => { setFrom(a); setTo(b); }} />
        <StoreChips all stores={s.stores} value={store} onChange={setStore} />
        <button className="link" onClick={list.reload}>Обновить</button>
        {list.loading && <Spinner />}
        {list.error && <div className="error">{list.error}</div>}
        {!list.loading && (list.data ?? []).length === 0 && <div className="empty">Ничего не найдено</div>}
        {(list.data ?? []).map((x) => (
          <div key={x.id} className="card click" onClick={() => nav(`/supply/${x.id}`)}>
            <div className="row" style={{ justifyContent: 'space-between' }}>
              <span className="title">{fmtDate(x.supplyDate)} · {supName(x.supplierId)}</span>
              <span className={x.hasReturn ? 'price low' : 'price'}>{x.hasReturn ? '−' : ''}{rub(x.total)}</span>
            </div>
            <div className="sub">{s.stores.find((st) => st.id === x.storeId)?.name} · {x.items.map((i) => `${i.name} (${i.qty})`).join(', ')}</div>
            <div className="sub">{x.hasReturn ? 'Возврат поставщику · ' : ''}{x.paid ? 'Оплачено' : 'Не оплачено'}</div>
          </div>
        ))}
      </main>
    </>
  );
}
