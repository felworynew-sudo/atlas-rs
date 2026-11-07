import { useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { api, type Category, type Product, type StockRow } from '../api';
import { Icon } from '../Icon';
import { num, rub } from '../money';
import { useSession } from '../session';
import { Header, Spinner, useLoad } from '../ui';

export default function Products() {
  const s = useSession();
  const nav = useNavigate();
  const [store, setStore] = useState<string>(s.storeId ?? '');
  const [cat, setCat] = useState<string>('all');
  const [search, setSearch] = useState('');
  const products = useLoad(() => api<Product[]>('/products', { query: { storeId: store, search } }), [store, search]);
  const stock = useLoad(() => (s.can('stock.view') ? api<StockRow[]>('/stocks', { query: { storeId: store } }) : Promise.resolve([] as StockRow[])), [store]);
  const cats = useLoad(() => api<Category[]>('/product-categories'), []);
  const stockMap = new Map((stock.data ?? []).map((r) => [r.productId, r]));

  const list = (products.data ?? []).filter((p) => cat === 'all' || p.categoryId === cat);
  // группы по категориям в порядке категорий; товары без категории — в конце
  const groups = [...(cats.data ?? []).map((c) => ({ id: c.id, name: c.name })), { id: '', name: 'Без категории' }]
    .map((g) => ({ ...g, items: list.filter((p) => (p.categoryId ?? '') === g.id) }))
    .filter((g) => g.items.length > 0);

  return (
    <>
      <Header title="Товары, услуги и ингредиенты" right={<button className="icon-btn" aria-label="Новый товар" onClick={() => nav('/product/new')}><Icon name="plus" /></button>} />
      <main className="page">
        <div className="chips">
          {s.stores.map((st) => <button key={st.id} className={`chip${store === st.id ? ' active' : ''}`} onClick={() => setStore(st.id)}>{st.name}</button>)}
        </div>
        <div className="row" style={{ marginBottom: 12, gap: 16 }}>
          <button className="link" onClick={() => nav('/products/batch')}>Массовое редактирование</button>
          {s.can('settings.export_import') && <button className="link" onClick={() => nav('/settings/export-import')}>Экспорт / Импорт</button>}
        </div>
        <input type="search" placeholder="Поиск" value={search} onChange={(e) => setSearch(e.target.value)} style={{ marginBottom: 14 }} />
        <div className="chips">
          <button className={`chip small${cat === 'all' ? ' active' : ''}`} onClick={() => setCat('all')}>Все</button>
          {(cats.data ?? []).map((c) => <button key={c.id} className={`chip small${cat === c.id ? ' active' : ''}`} onClick={() => setCat(c.id)}>{c.name}</button>)}
        </div>
        {products.loading && <Spinner />}
        {!products.loading && groups.length === 0 && <div className="empty">Ничего не найдено</div>}
        {groups.map((g) => (
          <section key={g.id}>
            <div className="day">{g.name}</div>
            {g.items.map((p) => {
              const price = p.stores.find((x) => x.storeId === store)?.salePrice ?? 0;
              const st = stockMap.get(p.id);
              return (
                <div key={p.id} className="card click row" onClick={() => nav(`/product/${p.id}`)}>
                  <div className="grow"><div className="title">{p.name}</div><div className="sub">{rub(price)}{p.isService ? ' · услуга' : ''}</div></div>
                  {st && !p.isService && <span className={st.low ? 'low' : 'muted'}>{num(st.qty)} шт</span>}
                </div>
              );
            })}
          </section>
        ))}
      </main>
    </>
  );
}
