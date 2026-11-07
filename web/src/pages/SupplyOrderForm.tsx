import { useEffect, useState } from 'react';
import { useNavigate, useParams, useSearchParams } from 'react-router-dom';
import { api, type StockRow, type Suggestion, type Supplier, type SupplyOrder } from '../api';
import { ORDER_STATUSES, todayLocal } from '../dates';
import { Icon } from '../Icon';
import { num, rub, round2 } from '../money';
import { ProductPicker } from '../ProductPicker';
import { useSession } from '../session';
import { Header, Spinner, StoreChips, useLoad, useToast } from '../ui';

interface Line { productId: string; name: string; qty: number; unitPrice: number }

/** Заказ поставщику: позиции с количеством на заказ, предложение от порога остатка, переход к приёмке. */
export default function SupplyOrderForm() {
  const { id = 'new' } = useParams();
  const isNew = id === 'new';
  const s = useSession();
  const nav = useNavigate();
  const [toast, say] = useToast();
  const sups = useLoad(() => api<Supplier[]>('/suppliers'), []);
  const existing = useLoad(() => (isNew ? Promise.resolve(null) : api<SupplyOrder>(`/supply-orders/${id}`)), [id]);
  const [params] = useSearchParams();
  const [store, setStore] = useState(params.get('storeId') ?? s.storeId ?? '');
  const stock = useLoad(() => (s.can('stock.view') && store ? api<StockRow[]>('/stocks', { query: { storeId: store } }) : Promise.resolve([] as StockRow[])), [store]);
  const stockMap = new Map((stock.data ?? []).map((r) => [r.productId, r]));
  const [supplier, setSupplier] = useState('');
  const [date, setDate] = useState(todayLocal());
  const [status, setStatus] = useState('draft');
  const [lines, setLines] = useState<Line[]>([]);
  const [picker, setPicker] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const o = existing.data;
    if (!o) return;
    setStore(o.storeId);
    setSupplier(o.supplierId ?? '');
    setDate(o.orderDate);
    setStatus(o.status);
    setLines(o.items.map((i) => ({ productId: i.productId, name: i.name, qty: i.qty, unitPrice: i.unitPrice })));
  }, [existing.data]);

  // переход из «+» категории на экране остатков: сразу предлагаем товары категории
  useEffect(() => {
    if (isNew && params.get('category') && store) void suggest();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const total = round2(lines.reduce((sum, l) => sum + l.qty * l.unitPrice, 0));
  const setQty = (i: number, qty: number) => setLines((ls) => ls.map((l, k) => (k === i ? { ...l, qty: Math.max(qty, 0) } : l)).filter((l) => l.qty > 0));

  async function suggest() {
    try {
      const r = await api<Suggestion[]>('/supply-orders/suggest', { query: { storeId: store, supplierId: supplier || undefined, categoryId: params.get('category') || undefined } });
      if (r.length === 0) say('Все товары выше порога остатка');
      setLines(r.map((x) => ({ productId: x.productId, name: x.name, qty: x.suggestedQty, unitPrice: x.unitPrice })));
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Ошибка');
    }
  }

  async function save() {
    setError(null);
    const body = {
      storeId: store,
      supplierId: supplier || undefined,
      status,
      orderDate: date,
      items: lines.map((l) => ({ productId: l.productId, qty: l.qty, unitPrice: l.unitPrice })),
    };
    try {
      if (isNew) await api('/supply-orders', { body });
      else await api(`/supply-orders/${id}`, { method: 'PUT', body });
      say('Успешно');
      setTimeout(() => nav('/supply-orders', { replace: true }), 700);
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Ошибка');
    }
  }

  async function remove() {
    if (!confirm('Удалить заказ?')) return;
    await api(`/supply-orders/${id}`, { method: 'DELETE' }).then(() => nav('/supply-orders', { replace: true })).catch((e: unknown) => say(e instanceof Error ? e.message : 'Ошибка'));
  }

  if (!isNew && existing.loading) return <><Header title="Заказ поставщику" /><Spinner /></>;
  const canReceive = !isNew && ['ordered', 'partially_received'].includes(status);

  return (
    <>
      <Header title={isNew ? 'Новый заказ' : 'Заказ поставщику'} right={!isNew && s.can('supply.delete') ? <button className="icon-btn" aria-label="Удалить заказ" onClick={remove}><Icon name="trash" /></button> : undefined} />
      <main className="page">
        <StoreChips stores={s.stores} value={store} onChange={setStore} />
        <label className="field"><span>Поставщик</span>
          <select value={supplier} onChange={(e) => setSupplier(e.target.value)}>
            <option value="">Без поставщика</option>
            {(sups.data ?? []).map((x) => <option key={x.id} value={x.id}>{x.name}</option>)}
          </select>
        </label>
        <label className="field"><span>Дата</span><input type="date" value={date} onChange={(e) => setDate(e.target.value)} /></label>
        <label className="field"><span>Статус</span>
          <select value={status} onChange={(e) => setStatus(e.target.value)}>
            {Object.entries(ORDER_STATUSES).map(([k, v]) => <option key={k} value={k}>{v}</option>)}
          </select>
        </label>

        <div className="h2">Товары</div>
        <div className="row wrap" style={{ marginBottom: 12 }}>
          <button className="btn light inline" onClick={() => setPicker(true)}>+ Добавить товар</button>
          <button className="btn light inline" onClick={suggest}>Предложить по порогу остатка</button>
        </div>
        {lines.length === 0 && <div className="muted">Добавьте товары или нажмите «Предложить по порогу остатка»</div>}
        {lines.map((l, i) => (
          <div key={l.productId} className="card">
            <div className="title">{l.name}</div>
            {stockMap.get(l.productId) && (() => {
              const r = stockMap.get(l.productId) as StockRow;
              return <div className="sub">Остаток {num(r.qty)} · продажи за неделю {num(r.sold7)} · списания за неделю {num(r.writtenOff7)} · порог {num(r.threshold || 1)}</div>;
            })()}
            <div className="row" style={{ marginTop: 8 }}>
              <span className="label" style={{ margin: 0 }}>Количество на заказ</span>
              <button className="qty-btn" aria-label={`Меньше ${l.name}`} onClick={() => setQty(i, l.qty - 1)}><Icon name="minus" size={16} /></button>
              <span style={{ minWidth: 28, textAlign: 'center' }}>{num(l.qty)}</span>
              <button className="qty-btn" aria-label={`Больше ${l.name}`} onClick={() => setQty(i, l.qty + 1)}><Icon name="plus" size={16} /></button>
              <span className="grow" style={{ textAlign: 'right' }}>{rub(l.qty * l.unitPrice)}</span>
            </div>
          </div>
        ))}
        <div className="total-row"><span>Сумма заказа</span><span>{rub(total)}</span></div>
        {error && <div className="error" role="alert">{error}</div>}
        <div style={{ display: 'grid', gap: 10 }}>
          {canReceive && <button className="btn light" onClick={() => nav(`/supply/new?order=${id}`)}>Принять</button>}
          <button className="btn" disabled={lines.length === 0 || !store} onClick={save}>Сохранить</button>
        </div>
      </main>
      {picker && (
        <ProductPicker
          storeId={store}
          skipServices
          onClose={() => setPicker(false)}
          onPick={(p) => {
            setLines((ls) => (ls.some((l) => l.productId === p.id) ? ls : [...ls, { productId: p.id, name: p.name, qty: 1, unitPrice: p.purchasePrice }]));
            setPicker(false);
          }}
        />
      )}
      {toast}
    </>
  );
}
