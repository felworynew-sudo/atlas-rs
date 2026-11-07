import { useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { api, type Category, type Product } from '../api';
import { useSession } from '../session';
import { Header, Spinner, StoreChips, useLoad, useToast } from '../ui';

interface Sup { id: string; name: string }
type Edit = Partial<{ purchasePrice: string; extraCost: string; salePrice: string; categoryId: string; supplierId: string; stockThreshold: string; important: boolean }>;
const n = (v: string): number => Number(v.replace(',', '.')) || 0;

/** Массовое редактирование: таблица товаров, изменённые строки сохраняются одним запросом (всё или ничего). */
export default function BatchEdit() {
  const s = useSession();
  const nav = useNavigate();
  const [toast, say] = useToast();
  const [store, setStore] = useState(s.storeId ?? '');
  const [search, setSearch] = useState('');
  const products = useLoad(() => api<Product[]>('/products', { query: { storeId: store } }), [store]);
  const cats = useLoad(() => api<Category[]>('/product-categories'), []);
  const sups = useLoad(() => api<Sup[]>('/suppliers'), []);
  const [edits, setEdits] = useState<Record<string, Edit>>({});
  const [error, setError] = useState<string | null>(null);
  const set = (id: string, patch: Edit) => setEdits((e) => ({ ...e, [id]: { ...e[id], ...patch } }));
  const changed = Object.keys(edits).length;

  async function save() {
    setError(null);
    const items = Object.entries(edits).map(([id, e]) => {
      const it: Record<string, unknown> = { id };
      if (e.purchasePrice !== undefined) it.purchasePrice = n(e.purchasePrice);
      if (e.extraCost !== undefined) it.extraCost = n(e.extraCost);
      if (e.stockThreshold !== undefined) it.stockThreshold = n(e.stockThreshold);
      if (e.categoryId !== undefined) it.categoryId = e.categoryId || null;
      if (e.supplierId !== undefined) it.supplierId = e.supplierId || null;
      if (e.important !== undefined) it.important = e.important;
      if (e.salePrice !== undefined) it.stores = [{ storeId: store, salePrice: n(e.salePrice) }];
      return it;
    });
    try {
      await api('/products/batch', { method: 'PATCH', body: { items } });
      setEdits({});
      products.reload();
      say('Успешно');
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Ошибка');
    }
  }

  const list = (products.data ?? []).filter((p) => p.name.toLowerCase().includes(search.toLowerCase()));
  const cell: React.CSSProperties = { maxWidth: 90 };
  return (
    <>
      <Header title="Массовое редактирование" right={<button className="link" onClick={() => nav('/products')}>К товарам</button>} />
      <main className="page wide">
        <StoreChips stores={s.stores} value={store} onChange={(id) => { setStore(id); setEdits({}); }} />
        <input type="search" placeholder="Поиск" value={search} onChange={(e) => setSearch(e.target.value)} style={{ marginBottom: 12 }} />
        {products.loading && <Spinner />}
        <div style={{ overflowX: 'auto' }}>
          <table className="table">
            <thead><tr><th>Товар</th><th>Закупка</th><th>Доп. расх.</th><th>Цена</th><th>Категория</th><th>Поставщик</th><th>Порог</th><th>Важный</th></tr></thead>
            <tbody>
              {list.map((p) => {
                const e = edits[p.id] ?? {};
                const price = p.stores.find((x) => x.storeId === store)?.salePrice ?? 0;
                return (
                  <tr key={p.id} style={e && Object.keys(e).length ? { background: 'var(--card-hover, rgba(255,255,255,0.05))' } : undefined}>
                    <td>{p.name}</td>
                    <td><input type="number" min="0" step="any" aria-label={`Закупка ${p.name}`} style={cell} value={e.purchasePrice ?? String(p.purchasePrice)} onChange={(ev) => set(p.id, { purchasePrice: ev.target.value })} disabled={p.isComposite} /></td>
                    <td><input type="number" min="0" step="any" aria-label={`Доп расходы ${p.name}`} style={cell} value={e.extraCost ?? String(p.extraCost)} onChange={(ev) => set(p.id, { extraCost: ev.target.value })} /></td>
                    <td><input type="number" min="0" step="any" aria-label={`Цена ${p.name}`} style={cell} value={e.salePrice ?? String(price)} onChange={(ev) => set(p.id, { salePrice: ev.target.value })} /></td>
                    <td>
                      <select aria-label={`Категория ${p.name}`} value={e.categoryId ?? p.categoryId ?? ''} onChange={(ev) => set(p.id, { categoryId: ev.target.value })}>
                        <option value="">—</option>
                        {(cats.data ?? []).map((c) => <option key={c.id} value={c.id}>{c.name}</option>)}
                      </select>
                    </td>
                    <td>
                      <select aria-label={`Поставщик ${p.name}`} value={e.supplierId ?? p.supplierId ?? ''} onChange={(ev) => set(p.id, { supplierId: ev.target.value })}>
                        <option value="">—</option>
                        {(sups.data ?? []).map((c) => <option key={c.id} value={c.id}>{c.name}</option>)}
                      </select>
                    </td>
                    <td><input type="number" min="0" step="any" aria-label={`Порог ${p.name}`} style={cell} value={e.stockThreshold ?? String(p.stockThreshold)} onChange={(ev) => set(p.id, { stockThreshold: ev.target.value })} /></td>
                    <td><input type="checkbox" aria-label={`Важный ${p.name}`} style={{ width: 20, height: 20 }} checked={e.important ?? p.important} onChange={(ev) => set(p.id, { important: ev.target.checked })} /></td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
        {error && <div className="error" role="alert">{error}</div>}
        <button className="btn" style={{ marginTop: 16 }} disabled={changed === 0} onClick={save}>Сохранить{changed ? ` (${changed})` : ''}</button>
      </main>
      {toast}
    </>
  );
}
