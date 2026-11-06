import { useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { api, type Supplier, type SupplyOrder } from '../api';
import { fmtDate, ORDER_STATUSES } from '../dates';
import { Icon } from '../Icon';
import { rub } from '../money';
import { useSession } from '../session';
import { Header, Spinner, StoreChips, useLoad } from '../ui';

export default function SupplyOrders() {
  const s = useSession();
  const nav = useNavigate();
  const [active, setActive] = useState(true);
  const [store, setStore] = useState('all');
  const list = useLoad(() => api<SupplyOrder[]>('/supply-orders', { query: { activeOnly: active, storeId: store === 'all' ? undefined : store } }), [active, store]);
  const sups = useLoad(() => api<Supplier[]>('/suppliers'), []);

  return (
    <>
      <Header title="Заказы поставщикам" right={<button className="icon-btn" aria-label="Новый заказ" onClick={() => nav('/supply-order/new')}><Icon name="plus" /></button>} />
      <main className="page">
        <label className="row" style={{ marginBottom: 16 }}>
          <input type="checkbox" checked={active} onChange={(e) => setActive(e.target.checked)} style={{ width: 22, height: 22 }} />
          <span>Показать активные</span>
        </label>
        <StoreChips all stores={s.stores} value={store} onChange={setStore} />
        {list.loading && <Spinner />}
        {!list.loading && (list.data ?? []).length === 0 && <div className="empty">Ничего не найдено</div>}
        {(list.data ?? []).map((o) => (
          <div key={o.id} className="card click" onClick={() => nav(`/supply-order/${o.id}`)}>
            <div className="row" style={{ justifyContent: 'space-between' }}>
              <span className="title">{fmtDate(o.orderDate)} · {(sups.data ?? []).find((x) => x.id === o.supplierId)?.name ?? 'Без поставщика'}</span>
              <span className="price">{rub(o.total)}</span>
            </div>
            <div className="sub">{ORDER_STATUSES[o.status] ?? o.status} · {o.items.length} поз.</div>
          </div>
        ))}
      </main>
    </>
  );
}
