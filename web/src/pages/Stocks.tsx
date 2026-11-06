import { useState } from 'react';
import { Link, useNavigate } from 'react-router-dom';
import { api, type Category, type Product, type StockRow, type StockSummary } from '../api';
import { num, rub } from '../money';
import { useSession } from '../session';
import { Header, Spinner, StoreChips, useLoad } from '../ui';

export default function Stocks() {
  const s = useSession();
  const nav = useNavigate();
  const [store, setStore] = useState(s.storeId ?? '');
  const [search, setSearch] = useState('');
  const stock = useLoad(() => api<StockRow[]>('/stocks', { query: { storeId: store } }), [store]);
  const summary = useLoad(() => api<StockSummary>('/stocks/summary', { query: { storeId: store } }), [store]);
  const products = useLoad(() => api<Product[]>('/products', { query: { storeId: store } }), [store]);
  const cats = useLoad(() => api<Category[]>('/product-categories'), []);

  const important = new Set((products.data ?? []).filter((p) => p.important).map((p) => p.id));
  const q = search.trim().toLowerCase();
  const rows = (stock.data ?? []).filter((r) => !q || r.name.toLowerCase().includes(q));
  const sections = [
    { id: '__important', name: 'Важные продукты', rows: rows.filter((r) => important.has(r.productId)) },
    ...[...(cats.data ?? []).map((c) => ({ id: c.id, name: c.name })), { id: '', name: 'Без категории' }].map((c) => ({
      ...c,
      rows: rows.filter((r) => (r.categoryId ?? '') === c.id),
    })),
  ].filter((g) => g.rows.length > 0);

  return (
    <>
      <Header title="Остатки товаров" />
      <main className="page">
        <div className="row wrap" style={{ marginBottom: 12 }}>
          {s.moduleOn('supplies') && s.can('supply.history') && <Link to="/supply-orders" className="link">Заказы поставщикам</Link>}
        </div>
        <StoreChips stores={s.stores} value={store} onChange={setStore} />
        <input type="search" placeholder="Поиск" value={search} onChange={(e) => setSearch(e.target.value)} style={{ marginBottom: 16 }} />
        {summary.data && (
          <div className="card">
            <div className="row" style={{ justifyContent: 'space-between' }}><span className="sub">Остаток в наличии по закупочной цене</span><b>{rub(summary.data.valueAtPurchase)}</b></div>
            <div className="row" style={{ justifyContent: 'space-between', marginTop: 6 }}><span className="sub">Остаток в наличии по продажной цене</span><b>{rub(summary.data.valueAtSale)}</b></div>
            <div className="row" style={{ justifyContent: 'space-between', marginTop: 6 }}><span className="sub">Товаров в наличии</span><b>{num(summary.data.unitsInStock)}</b></div>
          </div>
        )}
        {stock.loading && <Spinner />}
        {!stock.loading && sections.length === 0 && <div className="empty">Ничего не найдено</div>}
        {sections.map((g) => (
          <section key={g.id}>
            <div className="h2" style={{ fontSize: 20 }}>{g.name}</div>
            {g.rows.map((r) => (
              <div key={`${g.id}-${r.productId}`} className="card click row" onClick={() => nav(`/stock/${r.productId}?storeId=${store}`)}>
                <div className="grow">
                  <div className="title">{r.name}</div>
                  <div className="sub">7дн. продажи {num(r.sold7)} · 7дн. списания {num(r.writtenOff7)}</div>
                </div>
                <span className={r.low ? 'low price' : 'price'}>{num(r.qty)} шт</span>
              </div>
            ))}
          </section>
        ))}
      </main>
    </>
  );
}
