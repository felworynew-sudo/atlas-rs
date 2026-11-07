import { useEffect, useState } from 'react';
import { useNavigate, useParams, useSearchParams } from 'react-router-dom';
import { api, type Category, type Product, type Revision, type StockRow } from '../api';
import { fmtDateTime } from '../dates';
import { Icon } from '../Icon';
import { num, rub, round2 } from '../money';
import { useSession } from '../session';
import { Header, Spinner, StoreChips, useLoad, useToast } from '../ui';

interface Row { productId: string; name: string; system: number; fact: string; cost: number }

/** Новая ревизия по категории (остаток приводится к факту) и просмотр/удаление проведённой. */
export default function RevisionForm() {
  const { id = 'new' } = useParams();
  const isNew = id === 'new';
  const [params] = useSearchParams();
  const s = useSession();
  const nav = useNavigate();
  const [toast, say] = useToast();
  const cats = useLoad(() => api<Category[]>('/product-categories'), []);
  const existing = useLoad(() => (isNew ? Promise.resolve(null) : api<Revision>(`/revisions/${id}`)), [id]);
  const [store, setStore] = useState(params.get('storeId') ?? s.storeId ?? '');
  const [cat, setCat] = useState('');
  const [rows, setRows] = useState<Row[]>([]);
  const [loading, setLoading] = useState(false);
  const [comment, setComment] = useState('');
  const [error, setError] = useState<string | null>(null);
  const onlyProduct = params.get('product');

  // товары ревизии: выбранная категория (или один товар из карточки остатков) с системными остатками
  useEffect(() => {
    if (!isNew || (!cat && !onlyProduct)) {
      setRows([]);
      return;
    }
    setLoading(true);
    Promise.all([api<Product[]>('/products', { query: { storeId: store } }), api<StockRow[]>('/stocks', { query: { storeId: store } })])
      .then(([products, stock]) => {
        const qty = new Map(stock.map((r) => [r.productId, r.qty]));
        setRows(
          products
            .filter((p) => !p.isService && (onlyProduct ? p.id === onlyProduct : p.categoryId === cat))
            .map((p) => ({ productId: p.id, name: p.name, system: qty.get(p.id) ?? 0, fact: String(qty.get(p.id) ?? 0), cost: p.costPrice })),
        );
      })
      .catch((e: unknown) => setError(e instanceof Error ? e.message : 'Ошибка'))
      .finally(() => setLoading(false));
  }, [isNew, cat, store, onlyProduct]);

  const diff = (r: Row) => round2(((Number(r.fact.replace(',', '.')) || 0) - r.system) * r.cost);
  const total = round2(rows.reduce((sum, r) => sum + diff(r), 0));

  async function save() {
    setError(null);
    try {
      await api('/revisions', {
        body: {
          storeId: store,
          categoryId: cat || undefined,
          comment: comment || undefined,
          items: rows.map((r) => ({ productId: r.productId, factQty: Number(r.fact.replace(',', '.')) })),
        },
      });
      say('Успешно');
      setTimeout(() => nav('/revisions', { replace: true }), 700);
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Ошибка');
    }
  }

  async function remove() {
    if (!confirm('Удалить ревизию? Остатки вернутся к значениям до ревизии.')) return;
    await api(`/revisions/${id}`, { method: 'DELETE' }).then(() => nav('/revisions', { replace: true })).catch((e: unknown) => say(e instanceof Error ? e.message : 'Ошибка'));
  }

  if (!isNew) {
    if (existing.loading) return <><Header title="Ревизия" /><Spinner /></>;
    const d = existing.data;
    if (!d) return <><Header title="Ревизия" /><div className="empty">{existing.error ?? 'Ревизия не найдена'}</div></>;
    return (
      <>
        <Header title="Ревизия" right={s.can('revision.delete') ? <button className="icon-btn" aria-label="Удалить ревизию" onClick={remove}><Icon name="trash" /></button> : undefined} />
        <main className="page">
          <div className="card"><div className="title">{fmtDateTime(d.revisionDate)} · {s.stores.find((x) => x.id === d.storeId)?.name}</div></div>
          <table className="table">
            <thead><tr><th>Товар</th><th className="num">Кол-во</th><th className="num">Факт</th><th className="num">Сумма</th></tr></thead>
            <tbody>
              {d.items.map((i) => (
                <tr key={i.productId}><td>{i.name}</td><td className="num">{num(i.systemQty)}</td><td className="num">{num(i.factQty)}</td><td className={`num${i.diffAmount < 0 ? ' low' : ''}`}>{rub(i.diffAmount)}</td></tr>
              ))}
              <tr><td colSpan={3}><b>Итого</b></td><td className={`num${d.totalDiff < 0 ? ' low' : ''}`}><b>{rub(d.totalDiff)}</b></td></tr>
            </tbody>
          </table>
        </main>
        {toast}
      </>
    );
  }

  return (
    <>
      <Header title="Новая ревизия" />
      <main className="page">
        <StoreChips stores={s.stores} value={store} onChange={setStore} />
        {!onlyProduct && (
          <label className="field"><span>Категория</span>
            <select value={cat} onChange={(e) => setCat(e.target.value)}>
              <option value="">Выберите категорию</option>
              {(cats.data ?? []).map((c) => <option key={c.id} value={c.id}>{c.name}</option>)}
            </select>
          </label>
        )}
        {loading && <Spinner />}
        {rows.map((r, i) => (
          <div key={r.productId} className="card">
            <div className="title">{r.name}</div>
            <div className="row" style={{ marginTop: 8 }}>
              <div className="grow"><span className="label">Кол-во</span>{num(r.system)}</div>
              <label className="grow"><span className="label">Кол-во факт</span>
                <input type="number" min="0" step="any" aria-label={`Факт ${r.name}`} value={r.fact} onChange={(e) => setRows((rs) => rs.map((x, k) => (k === i ? { ...x, fact: e.target.value } : x)))} />
              </label>
              <div className="grow" style={{ textAlign: 'right' }}><span className="label">Сумма</span><span className={diff(r) < 0 ? 'low' : ''}>{rub(diff(r))}</span></div>
            </div>
          </div>
        ))}
        {rows.length > 0 && <div className="total-row"><span>Итого</span><span className={total < 0 ? 'low' : ''}>{rub(total)}</span></div>}
        {rows.length > 0 && <label className="field" style={{ marginTop: 12 }}><span>Комментарий</span><input type="text" value={comment} onChange={(e) => setComment(e.target.value)} /></label>}
        {error && <div className="error" role="alert">{error}</div>}
        <button className="btn" disabled={rows.length === 0 || rows.some((r) => r.fact.trim() === '')} onClick={save}>Сохранить</button>
      </main>
      {toast}
    </>
  );
}
