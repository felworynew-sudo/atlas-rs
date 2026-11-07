import { useState } from 'react';
import { Link, useNavigate } from 'react-router-dom';
import { api, type Category, type Product, type StockRow, type StockSummary } from '../api';
import { Icon } from '../Icon';
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

  const costOf = new Map((products.data ?? []).map((p) => [p.id, p.costPrice]));
  const priceOf = new Map((products.data ?? []).map((p) => [p.id, p.stores.find((x) => x.storeId === store)?.salePrice ?? 0]));
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
          <span className="muted" style={{ marginLeft: 12 }}>Нажмите «+» у категории, чтобы заказать товары ниже порога остатка</span>
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
            <div className="row" style={{ justifyContent: 'space-between', alignItems: 'center' }}>
              <div className="h2" style={{ fontSize: 20 }}>{g.name}</div>
              {g.id && g.id !== '__important' && s.moduleOn('supplies') && s.can('supply.history') && (
                <button className="icon-btn" aria-label={`Заказ поставщику: ${g.name}`} onClick={() => nav(`/supply-order/new?storeId=${store}&category=${g.id}`)}><Icon name="plus" /></button>
              )}
            </div>
            <div className="sub" style={{ marginTop: 0 }}>
              Остаток по закупке {rub(g.rows.reduce((a, r) => a + r.qty * (costOf.get(r.productId) ?? 0), 0))} · по продаже {rub(g.rows.reduce((a, r) => a + r.qty * (priceOf.get(r.productId) ?? 0), 0))}
            </div>
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
