import { useEffect, useState } from 'react';
import { useNavigate, useParams, useSearchParams } from 'react-router-dom';
import { api, type Product, type Supplier, type Supply, type SupplyOrder, type Wallet } from '../api';
import { fmtDate, todayLocal } from '../dates';
import { Icon } from '../Icon';
import { num, rub, round2 } from '../money';
import { ProductPicker } from '../ProductPicker';
import { useSession } from '../session';
import { printInvoice, type PrintTemplate } from '../printInvoice';
import { Header, Spinner, StoreChips, useLoad, useToast } from '../ui';

/** Уменьшает фото накладной до 1280 px по длинной стороне и сжимает в JPEG, чтобы запрос оставался небольшим. */
async function shrink(file: File): Promise<string> {
  const bmp = await createImageBitmap(file);
  const k = Math.min(1, 1280 / Math.max(bmp.width, bmp.height));
  const c = document.createElement('canvas');
  c.width = Math.round(bmp.width * k);
  c.height = Math.round(bmp.height * k);
  c.getContext('2d')?.drawImage(bmp, 0, 0, c.width, c.height);
  return c.toDataURL('image/jpeg', 0.7);
}

interface Line { productId: string; name: string; qty: string; price: string }

/** Новая приёмка (по товару или по заказу поставщику) и просмотр/удаление существующей. */
export default function SupplyForm() {
  const { id = 'new' } = useParams();
  const isNew = id === 'new';
  const [params] = useSearchParams();
  const s = useSession();
  const nav = useNavigate();
  const [toast, say] = useToast();
  const sups = useLoad(() => api<Supplier[]>('/suppliers'), []);
  const templates = useLoad(() => api<PrintTemplate[]>('/lists/print_templates'), []);
  const wallets = useLoad(() => api<Wallet[]>('/wallets'), []);
  const existing = useLoad(() => (isNew ? Promise.resolve(null) : api<Supply>(`/supplies/${id}`)), [id]);

  const [store, setStore] = useState(params.get('storeId') ?? s.storeId ?? '');
  const [supplier, setSupplier] = useState('');
  const [date, setDate] = useState(todayLocal());
  const [hasReturn, setHasReturn] = useState(false);
  const [pricesChanged, setPricesChanged] = useState(false);
  const [paid, setPaid] = useState(false);
  const [wallet, setWallet] = useState('');
  const [orderId, setOrderId] = useState<string | null>(params.get('order'));
  const [lines, setLines] = useState<Line[]>([]);
  const [picker, setPicker] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  // предзаполнение: товар из карточки остатков или заказ поставщику
  useEffect(() => {
    if (!isNew) return;
    const product = params.get('product');
    const order = params.get('order');
    if (product) {
      api<Product>(`/products/${product}`).then((p) => setLines([{ productId: p.id, name: p.name, qty: '1', price: String(p.purchasePrice) }])).catch(() => undefined);
    }
    if (order) {
      api<SupplyOrder>(`/supply-orders/${order}`)
        .then((o) => {
          setStore(o.storeId);
          setSupplier(o.supplierId ?? '');
          setOrderId(o.id);
          setLines(o.items.map((i) => ({ productId: i.productId, name: i.name, qty: String(i.qty), price: String(i.unitPrice) })));
        })
        .catch(() => undefined);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const total = round2(lines.reduce((sum, l) => sum + (Number(l.qty.replace(',', '.')) || 0) * (Number(l.price.replace(',', '.')) || 0), 0));
  const setLine = (i: number, patch: Partial<Line>) => setLines((ls) => ls.map((l, k) => (k === i ? { ...l, ...patch } : l)));

  const [photo, setPhoto] = useState('');
  const [comment, setComment] = useState('');
  const [barcode, setBarcode] = useState('');

  async function addByBarcode() {
    const code = barcode.trim();
    if (!code) return;
    try {
      const found = (await api<Product[]>('/products', { query: { storeId: store, search: code } })).find((p) => p.barcodes.includes(code) && !p.isService);
      if (!found) {
        setError('Товар с таким штрихкодом не найден');
        return;
      }
      setError(null);
      setLines((ls) => (ls.some((l) => l.productId === found.id) ? ls.map((l) => (l.productId === found.id ? { ...l, qty: String((Number(l.qty) || 0) + 1) } : l)) : [...ls, { productId: found.id, name: found.name, qty: '1', price: String(found.purchasePrice) }]));
      setBarcode('');
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Ошибка');
    }
  }

  async function save() {
    setBusy(true);
    setError(null);
    try {
      await api('/supplies', {
        body: {
          storeId: store,
          supplierId: supplier || undefined,
          orderId: orderId ?? undefined,
          supplyDate: date,
          hasReturn,
          pricesChanged,
          paid,
          invoicePhoto: photo || undefined,
          comment: comment || undefined,
          walletId: paid && wallet ? wallet : undefined,
          items: lines.map((l) => ({ productId: l.productId, qty: Number(l.qty.replace(',', '.')), price: Number(l.price.replace(',', '.')) })),
        },
      });
      say('Успешно');
      setTimeout(() => nav('/supplies', { replace: true }), 700);
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Ошибка');
    } finally {
      setBusy(false);
    }
  }

  async function remove() {
    if (!confirm('Удалить приёмку? Остатки вернутся к прежним значениям.')) return;
    try {
      await api(`/supplies/${id}`, { method: 'DELETE' });
      nav('/supplies', { replace: true });
    } catch (e) {
      say(e instanceof Error ? e.message : 'Ошибка');
    }
  }

  if (!isNew) {
    if (existing.loading) return <><Header title="Приёмка" /><Spinner /></>;
    const d = existing.data;
    if (!d) return <><Header title="Приёмка" /><div className="empty">{existing.error ?? 'Приёмка не найдена'}</div></>;
    return (
      <>
        <Header title="Приёмка" right={s.can('supply.delete') ? <button className="icon-btn" aria-label="Удалить приёмку" onClick={remove}><Icon name="trash" /></button> : undefined} />
        <main className="page">
          <div className="card">
            <div className="title">{fmtDate(d.supplyDate)} · {(sups.data ?? []).find((x) => x.id === d.supplierId)?.name ?? 'Без поставщика'}</div>
            <div className="sub">{d.hasReturn ? 'Возврат поставщику · ' : ''}{d.pricesChanged ? 'Цены обновлены · ' : ''}{d.paid ? 'Оплачено' : 'Не оплачено'}</div>
          </div>
          <table className="table">
            <thead><tr><th>Товар</th><th className="num">Кол-во</th><th className="num">Цена</th><th className="num">Сумма</th></tr></thead>
            <tbody>
              {d.items.map((i) => <tr key={i.productId}><td>{i.name}</td><td className="num">{num(i.qty)}</td><td className="num">{rub(i.price)}</td><td className="num">{rub(i.qty * i.price)}</td></tr>)}
              <tr><td colSpan={3}><b>Итого</b></td><td className="num"><b>{rub(d.total)}</b></td></tr>
            </tbody>
          </table>
          {d.comment && <div className="sub" style={{ marginTop: 8 }}>Комментарий: {d.comment}</div>}
          {d.invoicePhoto && <img src={d.invoicePhoto} alt="Фото накладной" style={{ maxWidth: '100%', borderRadius: 8, marginTop: 12 }} />}
          {(templates.data ?? []).length > 0 && (
            <div className="row" style={{ marginTop: 16, flexWrap: 'wrap' }}>
              {(templates.data ?? []).filter((t) => !t.params.storeId || t.params.storeId === d.storeId).map((t) => (
                <button key={t.id} className="btn light" onClick={() => {
                  const ok = printInvoice(t.params, { number: d.id.slice(0, 8).toUpperCase(), date: d.supplyDate, supplier: (sups.data ?? []).find((x) => x.id === d.supplierId)?.name ?? '—', items: d.items, total: d.total });
                  if (!ok) say('Разрешите всплывающие окна для печати');
                }}>Печать: {t.name}</button>
              ))}
            </div>
          )}
        </main>
        {toast}
      </>
    );
  }

  const storeWallets = (wallets.data ?? []).filter((w) => !w.storeId || w.storeId === store);

  return (
    <>
      <Header title="Новая приёмка" />
      <main className="page">
        <StoreChips stores={s.stores} value={store} onChange={setStore} />
        <label className="field"><span>Поставщик</span>
          <select value={supplier} onChange={(e) => setSupplier(e.target.value)}>
            <option value="">Без поставщика</option>
            {(sups.data ?? []).map((x) => <option key={x.id} value={x.id}>{x.name}</option>)}
          </select>
        </label>
        <label className="field"><span>Дата приемки</span><input type="date" value={date} onChange={(e) => setDate(e.target.value)} /></label>
        <div className="chips">
          <button className={`chip${!hasReturn ? ' active' : ''}`} onClick={() => setHasReturn(false)}>Без возврата</button>
          <button className={`chip${hasReturn ? ' active' : ''}`} onClick={() => setHasReturn(true)}>Возврат поставщику</button>
        </div>
        {!hasReturn && (
          <div className="chips">
            <button className={`chip${!pricesChanged ? ' active' : ''}`} onClick={() => setPricesChanged(false)}>Цены не изменились</button>
            <button className={`chip${pricesChanged ? ' active' : ''}`} onClick={() => setPricesChanged(true)}>Обновить закупочные цены</button>
          </div>
        )}

        <div className="h2">Товары</div>
        <button className="btn light" style={{ marginBottom: 12 }} onClick={() => setPicker(true)}>+ Добавить товар</button>
        <label className="field"><span>Штрихкод (Enter — добавить)</span>
          <input type="text" inputMode="numeric" value={barcode} placeholder="Введите штрих-код" onChange={(e) => setBarcode(e.target.value)} onKeyDown={(e) => { if (e.key === 'Enter') { e.preventDefault(); void addByBarcode(); } }} />
        </label>
        {lines.map((l, i) => (
          <div key={l.productId} className="card">
            <div className="row">
              <div className="grow title">{l.name}</div>
              <button className="icon-btn small" aria-label={`Убрать ${l.name}`} onClick={() => setLines((ls) => ls.filter((_, k) => k !== i))}><Icon name="close" size={18} /></button>
            </div>
            <div className="row" style={{ marginTop: 8 }}>
              <label className="grow"><span className="label">Кол-во</span><input type="number" min="0" step="any" aria-label={`Количество ${l.name}`} value={l.qty} onChange={(e) => setLine(i, { qty: e.target.value })} /></label>
              <label className="grow"><span className="label">Закупочная цена</span><input type="number" min="0" step="any" aria-label={`Цена ${l.name}`} value={l.price} onChange={(e) => setLine(i, { price: e.target.value })} /></label>
            </div>
          </div>
        ))}
        <div className="total-row"><span>Итого</span><span>{rub(total)}</span></div>

        <label className="row" style={{ marginBottom: 16 }}>
          <input type="checkbox" checked={paid} onChange={(e) => setPaid(e.target.checked)} style={{ width: 22, height: 22 }} />
          <span>{hasReturn ? 'Деньги за возврат получены' : 'Накладная оплачена'}</span>
        </label>
        {paid && (
          <label className="field"><span>Денежный счёт</span>
            <select value={wallet} onChange={(e) => setWallet(e.target.value)}>
              <option value="">Не указан</option>
              {storeWallets.map((w) => <option key={w.id} value={w.id}>{w.name}</option>)}
            </select>
          </label>
        )}
        <label className="field"><span>Фото накладной</span>
          <input type="file" accept="image/*" onChange={async (e) => {
            const f = e.target.files?.[0];
            if (f) setPhoto(await shrink(f).catch(() => ''));
          }} />
        </label>
        {photo && <img src={photo} alt="Фото накладной" style={{ maxWidth: '100%', borderRadius: 8, marginBottom: 12 }} />}
        <label className="field"><span>Комментарий</span><input type="text" value={comment} onChange={(e) => setComment(e.target.value)} /></label>
        {error && <div className="error" role="alert">{error}</div>}
        <button className="btn" disabled={busy || lines.length === 0 || !store} onClick={save}>Сохранить</button>
      </main>
      {picker && (
        <ProductPicker
          storeId={store}
          skipServices
          onClose={() => setPicker(false)}
          onPick={(p) => {
            setLines((ls) => (ls.some((l) => l.productId === p.id) ? ls : [...ls, { productId: p.id, name: p.name, qty: '1', price: String(p.purchasePrice) }]));
            setPicker(false);
          }}
        />
      )}
      {toast}
    </>
  );
}
