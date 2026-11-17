import { useState } from 'react';
import { api, type ClientOrder, type ListItem } from '../api';
import { CLIENT_ORDER_STATUSES, daysAgoLocal, endOfDay, fmtDateTime, startOfDay, todayLocal } from '../dates';
import { rub } from '../money';
import { useNavigate } from 'react-router-dom';
import { useSession } from '../session';
import { DateRange, Header, Modal, Spinner, StoreChips, useLoad, useToast } from '../ui';

const NEXT: Record<string, string[]> = {
  new: ['in_work', 'cancelled'],
  in_work: ['ready', 'cancelled'],
  ready: ['done', 'cancelled'],
  done: [],
  cancelled: ['new'],
};

/** Заказы от клиентов (с витрины): список, смена статуса, создание продажи из заказа. */
export default function ClientOrders() {
  const s = useSession();
  const nav = useNavigate();
  const [toast, say] = useToast();
  const [store, setStore] = useState('all');
  const [from, setFrom] = useState(daysAgoLocal(30));
  const [to, setTo] = useState(todayLocal());
  const [activeOnly, setActiveOnly] = useState(true);
  const [open, setOpen] = useState<ClientOrder | null>(null);
  const list = useLoad(
    () => api<ClientOrder[]>('/orders', { query: { storeId: store === 'all' ? undefined : store, from: startOfDay(from), to: endOfDay(to), activeOnly: activeOnly ? 'true' : undefined } }),
    [store, from, to, activeOnly],
  );
  const methods = useLoad(() => api<ListItem[]>('/lists/payment_methods'), []);
  const [method, setMethod] = useState('');
  const data = list.data ?? [];
  const total = data.reduce((sum, order) => sum + order.total, 0);

  async function run(fn: () => Promise<unknown>, ok = 'Успешно') {
    try {
      await fn();
      setOpen(null);
      list.reload();
      say(ok);
    } catch (e) {
      say(e instanceof Error ? e.message : 'Ошибка');
    }
  }

  return (
    <>
      <Header title="Заказы от клиентов" />
      <main className="page">
        <DateRange from={from} to={to} onChange={(a, b) => { setFrom(a); setTo(b); }} />
        <StoreChips all stores={s.stores} value={store} onChange={setStore} />
        <label className="row" style={{ marginBottom: 12, gap: 8 }}>
          <input type="checkbox" checked={activeOnly} onChange={(e) => setActiveOnly(e.target.checked)} style={{ width: 20, height: 20 }} />
          <span>Только необработанные и в работе</span>
        </label>
        {!list.loading && <div className="card"><b>Всего: {rub(total)}</b><div className="sub">{data.length} заказов в текущем фильтре</div></div>}
        {list.loading && <Spinner />}
        {list.error && <div className="error">{list.error}</div>}
        {!list.loading && data.length === 0 && <div className="empty">Заказов нет</div>}
        {data.map((o) => (
          <div key={o.id} className="card click" onClick={() => setOpen(o)}>
            <div className="row">
              <div className="grow">
                <div className="title">№{o.number} · {o.clientName || 'Без имени'}</div>
                <div className="sub">{fmtDateTime(o.createdAt)} · {CLIENT_ORDER_STATUSES[o.status] ?? o.status}{o.source ? ` · ${o.source}` : ''}</div>
              </div>
              <span className="price">{rub(o.total)}</span>
            </div>
          </div>
        ))}
      </main>
      {open && (
        <Modal onClose={() => setOpen(null)}>
          <div className="h2" style={{ marginTop: 0 }}>Заказ №{open.number} · {CLIENT_ORDER_STATUSES[open.status]}</div>
          <div className="sub">{open.clientName} {open.clientPhone && <a href={`tel:${open.clientPhone}`}>{open.clientPhone}</a>}</div>
          {open.deliveryAddress && <div className="sub">Адрес: {open.deliveryAddress}</div>}
          {open.paymentMethod && <div className="sub">Оплата: {open.paymentMethod}</div>}
          {open.notes && <div className="sub">Комментарий: {open.notes}</div>}
          <div style={{ margin: '12px 0' }}>
            {open.items.map((i, k) => <div key={k} className="total-row"><span>{i.name} × {i.qty}</span><span>{rub(i.price * i.qty)}</span></div>)}
            <div className="total-row"><b>Итого</b><b>{rub(open.total)}</b></div>
          </div>
          {open.saleId ? (
            <button className="btn light" onClick={() => nav(`/sale/${open.saleId}`)}>Открыть продажу</button>
          ) : (
            <>
              {open.status !== 'cancelled' && s.can('sale.new') && (
                <>
                  <label className="field"><span>Способ оплаты продажи</span>
                    <select value={method} onChange={(e) => setMethod(e.target.value)}>
                      <option value="">По умолчанию</option>
                      {(methods.data ?? []).map((m) => <option key={m.id} value={m.id}>{m.name}</option>)}
                    </select>
                  </label>
                  <button className="btn" onClick={() => run(() => api(`/orders/${open.id}/create-sale`, { body: { paymentMethodId: method || undefined } }), 'Продажа создана')}>Создать продажу</button>
                </>
              )}
              <div className="chips" style={{ marginTop: 12 }}>
                {(NEXT[open.status] ?? []).filter((st) => st !== 'done').map((st) => (
                  <button key={st} className="chip" onClick={() => run(() => api(`/orders/${open.id}/status`, { method: 'PATCH', body: { status: st } }))}>→ {CLIENT_ORDER_STATUSES[st]}</button>
                ))}
              </div>
            </>
          )}
        </Modal>
      )}
      {toast}
    </>
  );
}
