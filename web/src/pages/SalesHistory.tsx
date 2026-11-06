import { useMemo, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { api, type ListItem, type Sale } from '../api';
import { rub } from '../money';
import { useSession } from '../session';
import { Header, Spinner, useLoad } from '../ui';

const todayIso = () => new Date().toISOString().slice(0, 10);
// Границы выбранных дней в местном времени браузера, в UTC для сервера.
const startOf = (d: string) => new Date(`${d}T00:00:00`).toISOString();
const endOf = (d: string) => new Date(new Date(`${d}T00:00:00`).getTime() + 86_400_000).toISOString();

const weekdays = ['воскресенье', 'понедельник', 'вторник', 'среда', 'четверг', 'пятница', 'суббота'];
const dayTitle = (d: Date) => `${d.getDate()}/${d.getMonth() + 1}/${d.getFullYear()} ${weekdays[d.getDay()]}`;
const time = (d: Date) => d.toLocaleTimeString('ru-RU', { hour: '2-digit', minute: '2-digit' });

export default function SalesHistory() {
  const s = useSession();
  const nav = useNavigate();
  const [from, setFrom] = useState(todayIso());
  const [to, setTo] = useState(todayIso());
  const [store, setStore] = useState<string>('all');
  const pay = useLoad(() => api<ListItem[]>('/lists/payment_methods'), []);
  const sales = useLoad(
    () => api<Sale[]>('/sales', { query: { from: startOf(from), to: endOf(to), storeId: store === 'all' ? undefined : store } }),
    [from, to, store],
  );
  const payName = (id: string | null) => (pay.data ?? []).find((p) => p.id === id)?.name ?? '—';
  const storeName = (id: string) => s.stores.find((x) => x.id === id)?.name ?? '';

  const groups = useMemo(() => {
    const m = new Map<string, Sale[]>();
    for (const sale of sales.data ?? []) {
      const key = dayTitle(new Date(sale.saleDate));
      m.set(key, [...(m.get(key) ?? []), sale]);
    }
    return [...m.entries()];
  }, [sales.data]);

  return (
    <>
      <Header title="История продаж" />
      <main className="page">
        <span className="label">Фильтр по дате</span>
        <div className="row" style={{ marginBottom: 16 }}>
          <input type="date" aria-label="С" value={from} max={to} onChange={(e) => setFrom(e.target.value)} />
          <input type="date" aria-label="По" value={to} min={from} onChange={(e) => setTo(e.target.value)} />
        </div>
        <div className="chips">
          <button className={`chip${store === 'all' ? ' active' : ''}`} onClick={() => setStore('all')}>Все</button>
          {s.stores.map((st) => <button key={st.id} className={`chip${store === st.id ? ' active' : ''}`} onClick={() => setStore(st.id)}>{st.name}</button>)}
        </div>
        <button className="link" onClick={sales.reload}>Обновить</button>

        {sales.loading && <Spinner />}
        {sales.error && <div className="error">{sales.error}</div>}
        {!sales.loading && groups.length === 0 && <div className="empty">Ничего не найдено</div>}
        {groups.map(([day, list]) => (
          <section key={day}>
            <div className="day">{day}</div>
            {list.map((sale) => (
              <div key={sale.id} className="card click" onClick={() => nav(`/sale/${sale.id}`)}>
                <div className="row" style={{ justifyContent: 'space-between' }}>
                  <span className="title">{time(new Date(sale.saleDate))} · {payName(sale.paymentMethodId)}</span>
                  <span className="price">{rub(sale.total)}</span>
                </div>
                <div className="sub">{storeName(sale.storeId)} · {sale.userName}</div>
                <div className="sub">{sale.items.map((i) => `${i.name} (${i.qty})`).join(', ')}</div>
                {sale.returnedAmount > 0 && <div className="sub low">Возврат {rub(sale.returnedAmount)}</div>}
              </div>
            ))}
          </section>
        ))}
      </main>
    </>
  );
}
