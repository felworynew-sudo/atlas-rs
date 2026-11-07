import { useEffect, useMemo, useState } from 'react';
import { useNavigate, useParams, useSearchParams } from 'react-router-dom';
import { api, type Movement, type Product, type StockRow } from '../api';
import { daysAgoLocal, endOfDay, fmtDate, fmtDateTime, MOVEMENT_KINDS, startOfDay, todayLocal } from '../dates';
import { num } from '../money';
import { useSession } from '../session';
import { DateRange, Header, Spinner, StoreChips, useLoad, useToast } from '../ui';

/** Карточка товара «История движения»: пороги, быстрые действия и журнал остатков. */
export default function StockCard() {
  const { id = '' } = useParams();
  const [params] = useSearchParams();
  const s = useSession();
  const nav = useNavigate();
  const [toast, say] = useToast();
  const [store, setStore] = useState(params.get('storeId') ?? s.storeId ?? '');
  const product = useLoad(() => api<Product>(`/products/${id}`), [id]);
  const stock = useLoad(() => api<StockRow[]>('/stocks', { query: { storeId: store } }), [store]);
  const [from, setFrom] = useState(daysAgoLocal(30));
  const [to, setTo] = useState(todayLocal());
  const moves = useLoad(() => api<Movement[]>(`/stocks/${id}/movements`, { query: { storeId: store, from: startOfDay(from), to: endOfDay(to) } }), [id, store, from, to]);
  const [important, setImportant] = useState(false);
  const [threshold, setThreshold] = useState('');

  useEffect(() => {
    if (product.data) {
      setImportant(product.data.important);
      setThreshold(String(product.data.stockThreshold || ''));
    }
  }, [product.data]);

  const current = stock.data?.find((r) => r.productId === id)?.qty ?? 0;
  // баланс периода: сумма изменений по видам событий
  const totals = useMemo(() => {
    const m = new Map<string, number>();
    for (const e of moves.data ?? []) m.set(e.kind, (m.get(e.kind) ?? 0) + e.delta);
    return [...m.entries()];
  }, [moves.data]);

  // журнал отсортирован от новых к старым: начало периода — «было» у самого старого движения, конец — «стало» у самого нового
  const opening = (moves.data ?? []).length > 0 ? (moves.data ?? [])[(moves.data ?? []).length - 1].qtyBefore : current;
  const closing = (moves.data ?? []).length > 0 ? (moves.data ?? [])[0].qtyAfter : current;

  async function save() {
    try {
      await api('/products/batch', { method: 'PATCH', body: { items: [{ id, important, stockThreshold: Number(threshold.replace(',', '.')) || 0 }] } });
      say('Успешно');
    } catch (e) {
      say(e instanceof Error ? e.message : 'Ошибка');
    }
  }

  if (product.loading) return <><Header title="Товар" /><Spinner /></>;
  if (!product.data) return <><Header title="Товар" /><div className="empty">{product.error ?? 'Товар не найден'}</div></>;
  const q = `?product=${id}&storeId=${store}`;

  return (
    <>
      <Header title={product.data.name} />
      <main className="page">
        <StoreChips stores={s.stores} value={store} onChange={setStore} />
        <div className="org-name" style={{ textAlign: 'left', fontSize: 30, margin: '8px 0 16px' }}>{num(current)} шт</div>
        <div style={{ display: 'grid', gap: 10 }}>
          {s.moduleOn('supplies') && s.can('supply.history') && <button className="btn light" onClick={() => nav(`/supply/new${q}`)}>Создать приемку</button>}
          {s.moduleOn('writeoffs') && s.can('writeoff.history') && <button className="btn light" onClick={() => nav(`/writeoff/new${q}`)}>Сделать списание</button>}
          {s.moduleOn('revisions') && s.can('revision.view') && <button className="btn light" onClick={() => nav(`/revision/new${q}`)}>Сделать ревизию</button>}
        </div>

        <div className="h2">Данные по товару</div>
        <label className="row" style={{ marginBottom: 16 }}>
          <input type="checkbox" checked={important} onChange={(e) => setImportant(e.target.checked)} style={{ width: 22, height: 22 }} />
          <span>Показывать в важных товарах</span>
        </label>
        <label className="field"><span>Количество при котором нужно подсветить красным</span>
          <input type="number" min="0" step="any" value={threshold} placeholder="1" onChange={(e) => setThreshold(e.target.value)} />
        </label>
        <button className="btn" onClick={save}>Сохранить</button>

        <div className="h2">История движения товара</div>
        <DateRange from={from} to={to} onChange={(a, b) => { setFrom(a); setTo(b); }} />
        {moves.loading && <Spinner />}
        {totals.length > 0 && (
          <div className="card">
            <div className="row" style={{ justifyContent: 'space-between' }}><span>Количество на начало периода</span><b>{num(opening)}</b></div>
            {totals.map(([kind, sum]) => (
              <div key={kind} className="row" style={{ justifyContent: 'space-between' }}>
                <span>{MOVEMENT_KINDS[kind] ?? kind}</span>
                <b className={sum < 0 ? 'low' : ''}>{sum > 0 ? '+' : ''}{num(sum)}</b>
              </div>
            ))}
            <div className="row" style={{ justifyContent: 'space-between', marginTop: 8 }}><b>Итого к концу периода</b><b>{num(closing)}</b></div>
            <div className="row" style={{ justifyContent: 'space-between', marginTop: 4 }}><span className="sub">Текущее количество в системе</span><span className="sub">{num(current)}</span></div>
          </div>
        )}
        {!moves.loading && (moves.data ?? []).length === 0 && <div className="empty">Движений пока нет</div>}
        {(moves.data ?? []).map((e, i, arr) => (
          <div key={e.id}>
            {(i === 0 || fmtDate(arr[i - 1].createdAt) !== fmtDate(e.createdAt)) && <div className="day">{fmtDate(e.createdAt)}</div>}
          <div className="card">
            <div className="row" style={{ justifyContent: 'space-between' }}>
              <span className="title">{MOVEMENT_KINDS[e.kind] ?? e.kind}</span>
              <span className={e.delta < 0 ? 'low price' : 'price'}>{e.delta > 0 ? '+' : ''}{num(e.delta)}</span>
            </div>
            <div className="sub">{fmtDateTime(e.createdAt)} · Предыдущее значение {num(e.qtyBefore)}</div>
          </div>
          </div>
        ))}
      </main>
      {toast}
    </>
  );
}
