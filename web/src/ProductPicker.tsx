import { useState } from 'react';
import { api, type Product, type StockRow } from './api';
import { num, rub } from './money';
import { useSession } from './session';
import { Modal, Spinner, useLoad } from './ui';

/** Выбор товара с поиском: название, цена филиала и остаток. */
export function ProductPicker({
  storeId,
  onPick,
  onClose,
  skipServices = false,
}: {
  storeId: string;
  onPick: (p: Product) => void;
  onClose: () => void;
  skipServices?: boolean;
}) {
  const s = useSession();
  const [search, setSearch] = useState('');
  const products = useLoad(() => api<Product[]>('/products', { query: { storeId, search } }), [storeId, search]);
  const stock = useLoad(() => (s.can('stock.view') ? api<StockRow[]>('/stocks', { query: { storeId } }) : Promise.resolve([] as StockRow[])), [storeId]);
  const stockMap = new Map((stock.data ?? []).map((r) => [r.productId, r]));
  const list = (products.data ?? []).filter((p) => !(skipServices && p.isService)).slice(0, 60);

  return (
    <Modal onClose={onClose}>
      <div className="h2" style={{ marginTop: 0 }}>Выбор товара</div>
      <input type="search" autoFocus placeholder="Поиск по названию или штрихкоду" value={search} onChange={(e) => setSearch(e.target.value)} style={{ marginBottom: 12 }} />
      {products.loading && <Spinner />}
      {!products.loading && list.length === 0 && <div className="empty">Ничего не найдено</div>}
      {list.map((p) => {
        const st = stockMap.get(p.id);
        const price = p.stores.find((x) => x.storeId === storeId)?.salePrice ?? 0;
        return (
          <div key={p.id} className="card click row" onClick={() => onPick(p)}>
            <div className="grow"><div className="title">{p.name}</div><div className="sub">{rub(price)}</div></div>
            {st && !p.isService && <span className={st.low ? 'low' : 'muted'}>{num(st.qty)} шт</span>}
          </div>
        );
      })}
    </Modal>
  );
}
