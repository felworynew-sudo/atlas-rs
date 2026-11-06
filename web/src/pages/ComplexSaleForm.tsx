import { useEffect, useState } from 'react';
import { useNavigate, useParams } from 'react-router-dom';
import { api, type Client, type ComplexSale, type ComplexTemplate, type Wallet } from '../api';
import { COMPLEX_STATUSES, fmtDateTime } from '../dates';
import { Icon } from '../Icon';
import { num, rub } from '../money';
import { ProductPicker } from '../ProductPicker';
import { useSession } from '../session';
import { Header, Spinner, StoreChips, useLoad, useToast } from '../ui';

interface Line { productId: string | null; name: string; qty: string; cost: string; extraCost: string }
const toNum = (v: string): number => Number(v.replace(',', '.')) || 0;

/** Комплексная продажа: шаблон, клиент, статус, расходы (товары со склада и свободные), цена, оплаты и комментарии. */
export default function ComplexSaleForm() {
  const { id } = useParams();
  const isNew = id === 'new';
  const s = useSession();
  const nav = useNavigate();
  const [toast, say] = useToast();
  const existing = useLoad(() => (isNew ? Promise.resolve(null) : api<ComplexSale>(`/complex-sales/${id}`)), [id]);
  const templates = useLoad(() => api<ComplexTemplate[]>('/complex-sale-templates'), []);
  const clients = useLoad(() => (s.can('client.view') ? api<Client[]>('/clients') : Promise.resolve([] as Client[])), []);
  const wallets = useLoad(() => api<Wallet[]>('/wallets'), []);

  const [store, setStore] = useState(s.storeId ?? '');
  const [name, setName] = useState('');
  const [tplName, setTplName] = useState('');
  const [client, setClient] = useState('');
  const [status, setStatus] = useState('new');
  const [comment, setComment] = useState('');
  const [price, setPrice] = useState('');
  const [lines, setLines] = useState<Line[]>([]);
  const [picker, setPicker] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [payAmount, setPayAmount] = useState('');
  const [payWallet, setPayWallet] = useState('');
  const [note, setNote] = useState('');

  const sale = existing.data;
  useEffect(() => {
    if (!sale) return;
    setStore(sale.storeId);
    setName(sale.name);
    setTplName(sale.templateName);
    setClient(sale.clientId ?? '');
    setStatus(sale.status);
    setComment(sale.comment);
    setPrice(String(sale.finalPrice));
    setLines(sale.items.map((i) => ({ productId: i.productId, name: i.name, qty: String(i.qty), cost: String(i.cost), extraCost: String(i.extraCost) })));
  }, [sale]);

  const patch = (i: number, p: Partial<Line>) => setLines((cur) => cur.map((x, k) => (k === i ? { ...x, ...p } : x)));
  const expenses = lines.reduce((sum, l) => sum + toNum(l.qty) * toNum(l.cost) + toNum(l.extraCost), 0);
  const profit = toNum(price) - expenses;

  function applyTemplate(tid: string) {
    const t = (templates.data ?? []).find((x) => x.id === tid);
    if (!t) return;
    setTplName(t.name);
    if (!name) setName(t.name);
    setPrice(String(t.finalPrice));
    setLines(t.items.map((i) => ({ productId: i.productId ?? null, name: i.name, qty: String(i.qty ?? 1), cost: String(i.cost ?? 0), extraCost: String(i.extraCost ?? 0) })));
  }

  async function save() {
    setError(null);
    const body = {
      storeId: store,
      clientId: client || null,
      templateName: tplName || undefined,
      name,
      status,
      comment,
      finalPrice: toNum(price),
      items: lines.map((l) => ({ productId: l.productId, name: l.name, qty: toNum(l.qty), cost: l.cost === '' ? undefined : toNum(l.cost), extraCost: toNum(l.extraCost) })),
    };
    try {
      if (isNew) {
        const created = await api<ComplexSale>('/complex-sales', { body });
        nav(`/complex-sale/${created.id}`, { replace: true });
      } else {
        await api(`/complex-sales/${id}`, { method: 'PUT', body });
        existing.reload();
      }
      say('Успешно');
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Ошибка');
    }
  }

  async function act(fn: () => Promise<unknown>) {
    try {
      await fn();
      existing.reload();
    } catch (e) {
      say(e instanceof Error ? e.message : 'Ошибка');
    }
  }

  const addPayment = () => act(async () => {
    await api(`/complex-sales/${id}/payments`, { body: { amount: toNum(payAmount), walletId: payWallet || undefined } });
    setPayAmount('');
  });
  const addNote = () => act(async () => {
    await api(`/complex-sales/${id}/comments`, { body: { text: note } });
    setNote('');
  });
  const removeSale = async () => {
    if (!confirm('Удалить комплексную продажу? Материалы вернутся на склад.')) return;
    await act(async () => {
      await api(`/complex-sales/${id}`, { method: 'DELETE' });
      nav('/complex-sales', { replace: true });
    });
  };

  const storeWallets = (wallets.data ?? []).filter((w) => !w.storeId || w.storeId === store);
  if (!isNew && existing.loading && !sale) return <Spinner />;

  return (
    <>
      <Header title={isNew ? 'Новая комплексная продажа' : name || 'Комплексная продажа'} right={!isNew && s.can('complex_sale.delete') ? <button className="icon-btn" aria-label="Удалить" onClick={removeSale}><Icon name="trash" /></button> : undefined} />
      <main className="page">
        {isNew && <StoreChips stores={s.stores} value={store} onChange={setStore} />}
        {(templates.data ?? []).length > 0 && (
          <label className="field"><span>Шаблон</span>
            <select value="" onChange={(e) => applyTemplate(e.target.value)}>
              <option value="">{tplName || 'Без шаблона'}</option>
              {(templates.data ?? []).map((t) => <option key={t.id} value={t.id}>{t.name}</option>)}
            </select>
          </label>
        )}
        <label className="field"><span>Название</span><input type="text" value={name} onChange={(e) => setName(e.target.value)} /></label>
        <label className="field"><span>Клиент</span>
          <select value={client} onChange={(e) => setClient(e.target.value)}>
            <option value="">Без клиента</option>
            {(clients.data ?? []).map((c) => <option key={c.id} value={c.id}>{c.name}</option>)}
          </select>
        </label>
        <label className="field"><span>Статус</span>
          <select value={status} onChange={(e) => setStatus(e.target.value)}>
            {Object.entries(COMPLEX_STATUSES).map(([k, v]) => <option key={k} value={k}>{v}</option>)}
          </select>
        </label>

        <div className="h2">Расходы</div>
        {lines.map((l, i) => (
          <div key={i} className="card">
            <div className="row">
              <input type="text" aria-label="Название" value={l.name} placeholder="Название" readOnly={!!l.productId} onChange={(e) => patch(i, { name: e.target.value })} />
              <button className="icon-btn small" aria-label="Удалить строку" onClick={() => setLines((cur) => cur.filter((_, k) => k !== i))}><Icon name="trash" size={18} /></button>
            </div>
            <div className="row" style={{ marginTop: 8 }}>
              <label className="field grow"><span>Кол-во</span><input type="number" min="0" step="any" value={l.qty} onChange={(e) => patch(i, { qty: e.target.value })} /></label>
              <label className="field grow"><span>Себестоимость</span><input type="number" min="0" step="any" value={l.cost} onChange={(e) => patch(i, { cost: e.target.value })} /></label>
              <label className="field grow"><span>Доп. расход</span><input type="number" min="0" step="any" value={l.extraCost} onChange={(e) => patch(i, { extraCost: e.target.value })} /></label>
            </div>
          </div>
        ))}
        <div className="row" style={{ marginBottom: 16 }}>
          <button className="btn light" onClick={() => setPicker(true)}>+ Товар со склада</button>
          <button className="btn light" onClick={() => setLines([...lines, { productId: null, name: '', qty: '1', cost: '0', extraCost: '0' }])}>+ Свой расход</button>
        </div>

        <label className="field"><span>Стоимость для клиента</span><input type="number" min="0" step="any" value={price} placeholder="0" onChange={(e) => setPrice(e.target.value)} /></label>
        <div className="total-row"><span>Расходы</span><span>{rub(expenses)}</span></div>
        <div className="total-row"><span>Ожидаемая прибыль</span><span>{rub(profit)}</span></div>
        <label className="field"><span>Комментарий</span><textarea rows={2} value={comment} onChange={(e) => setComment(e.target.value)} /></label>
        {error && <div className="error" role="alert">{error}</div>}
        <button className="btn" disabled={!name.trim() || !store} onClick={save}>Сохранить</button>

        {sale && (
          <>
            <div className="h2">Оплата</div>
            <div className="total-row"><span>Оплачено</span><span>{rub(sale.paid)}</span></div>
            <div className="total-row"><span>Осталось</span><span>{rub(sale.remaining)}</span></div>
            {sale.payments.map((p) => (
              <div key={p.id} className="card row">
                <div className="grow"><div className="title">{rub(p.amount)}</div><div className="sub">{fmtDateTime(p.paidAt)} · {(wallets.data ?? []).find((w) => w.id === p.walletId)?.name ?? 'без счёта'}</div></div>
                {s.can('complex_sale.delete') && <button className="icon-btn small" aria-label="Удалить оплату" onClick={() => act(() => api(`/complex-sales/${id}/payments/${p.id}`, { method: 'DELETE' }))}><Icon name="trash" size={18} /></button>}
              </div>
            ))}
            {sale.remaining > 0 && (
              <>
                <div className="row">
                  <label className="field grow"><span>Сумма</span><input type="number" min="0" step="any" value={payAmount} placeholder={num(sale.remaining)} onChange={(e) => setPayAmount(e.target.value)} /></label>
                  <label className="field grow"><span>Счёт</span>
                    <select value={payWallet} onChange={(e) => setPayWallet(e.target.value)}>
                      <option value="">Без счёта</option>
                      {storeWallets.map((w) => <option key={w.id} value={w.id}>{w.name}</option>)}
                    </select>
                  </label>
                </div>
                <button className="btn light" disabled={!(toNum(payAmount) > 0)} onClick={addPayment}>Принять оплату</button>
              </>
            )}

            <div className="h2">История</div>
            {sale.updates.map((u, i) => <div key={i} className="sub" style={{ marginBottom: 6 }}>{fmtDateTime(u.createdAt)} — {u.text}</div>)}
            <div className="row">
              <input type="text" aria-label="Комментарий" placeholder="Добавить комментарий" value={note} onChange={(e) => setNote(e.target.value)} />
              <button className="btn light" style={{ width: 'auto' }} disabled={!note.trim()} onClick={addNote}>OK</button>
            </div>
          </>
        )}
      </main>
      {picker && (
        <ProductPicker
          storeId={store}
          onClose={() => setPicker(false)}
          onPick={(p) => {
            setLines((cur) => [...cur, { productId: p.id, name: p.name, qty: '1', cost: '', extraCost: '0' }]);
            setPicker(false);
          }}
        />
      )}
      {toast}
    </>
  );
}
