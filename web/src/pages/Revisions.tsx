import { useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { api, type Revision } from '../api';
import { endOfDay, fmtDateTime, startOfDay, todayLocal } from '../dates';
import { Icon } from '../Icon';
import { rub } from '../money';
import { useSession } from '../session';
import { DateRange, Header, Spinner, StoreChips, useLoad } from '../ui';

export default function Revisions() {
  const s = useSession();
  const nav = useNavigate();
  const [from, setFrom] = useState(todayLocal());
  const [to, setTo] = useState(todayLocal());
  const [store, setStore] = useState('all');
  const list = useLoad(() => api<Revision[]>('/revisions', { query: { from: startOfDay(from), to: endOfDay(to), storeId: store === 'all' ? undefined : store } }), [from, to, store]);

  return (
    <>
      <Header title="Ревизии" right={<button className="icon-btn" aria-label="Новая ревизия" onClick={() => nav('/revision/new')}><Icon name="plus" /></button>} />
      <main className="page">
        <DateRange from={from} to={to} onChange={(a, b) => { setFrom(a); setTo(b); }} />
        <StoreChips all stores={s.stores} value={store} onChange={setStore} />
        <button className="link" onClick={list.reload}>Обновить</button>
        {list.loading && <Spinner />}
        {!list.loading && (list.data ?? []).length === 0 && <div className="empty">Ничего не найдено</div>}
        {(list.data ?? []).map((r) => (
          <div key={r.id} className="card click" onClick={() => nav(`/revision/${r.id}`)}>
            <div className="row" style={{ justifyContent: 'space-between' }}>
              <span className="title">{fmtDateTime(r.revisionDate)} · {s.stores.find((x) => x.id === r.storeId)?.name}</span>
              <span className={r.totalDiff < 0 ? 'price low' : 'price'}>{r.totalDiff > 0 ? '+' : ''}{rub(r.totalDiff)}</span>
            </div>
            <div className="sub">{r.items.length} поз. · {r.items.filter((i) => i.factQty !== i.systemQty).length} с расхождением</div>
          </div>
        ))}
      </main>
    </>
  );
}
