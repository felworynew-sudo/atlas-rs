import { useState } from 'react';
import { api, type ListItem, type Wallet } from '../api';
import { daysAgoLocal, endOfDay, fmtDate, startOfDay, todayLocal } from '../dates';
import { Icon } from '../Icon';
import { rub } from '../money';
import { useSession } from '../session';
import { DateRange, Header, Modal, Spinner, StoreChips, useLoad, useToast } from '../ui';

interface Payment { id: string; storeId: string; categoryName: string | null; walletId: string; amount: number; paidOn: string; periodFrom: string | null; periodTo: string | null; details: string }
interface MoneyReport { wallets: { walletId: string; name: string; opening: number; inflow: number; outflow: number; closing: number; byKind: Record<string, { in: number; out: number }> }[] }

/** Платежи (аренда, коммуналка и т.п. за период) и отчёт о движении денег по счетам. */
export default function Payments() {
  const s = useSession();
  const [toast, say] = useToast();
  const [tab, setTab] = useState<'list' | 'report'>('list');
  const [store, setStore] = useState('all');
  const [from, setFrom] = useState(daysAgoLocal(30));
  const [to, setTo] = useState(todayLocal());
  const [form, setForm] = useState(false);
  const [by, setBy] = useState<'created' | 'period'>('created');
  const list = useLoad(() => api<Payment[]>('/payments', { query: { storeId: store === 'all' ? undefined : store, from, to, by } }), [store, from, to, by]);
  const report = useLoad(() => (tab === 'report' ? api<MoneyReport>('/payments/report', { query: { from: startOfDay(from), to: endOfDay(to) } }) : Promise.resolve(null)), [tab, from, to]);
  const wallets = useLoad(() => api<Wallet[]>('/wallets'), []);
  const total = (list.data ?? []).reduce((a, x) => a + x.amount, 0);

  async function remove(id: string) {
    if (!confirm('Удалить платёж? Деньги вернутся на счёт.')) return;
    try {
      await api(`/payments/${id}`, { method: 'DELETE' });
      list.reload();
    } catch (e) {
      say(e instanceof Error ? e.message : 'Ошибка');
    }
  }

  return (
    <>
      <Header title="Платежи" right={<button className="icon-btn" aria-label="Новый платёж" onClick={() => setForm(true)}><Icon name="plus" /></button>} />
      <main className="page">
        <div className="chips">
          <button className={`chip${tab === 'list' ? ' active' : ''}`} onClick={() => setTab('list')}>Платежи</button>
          <button className={`chip${tab === 'report' ? ' active' : ''}`} onClick={() => setTab('report')}>Движение денег</button>
        </div>
        <DateRange from={from} to={to} onChange={(a, b) => { setFrom(a); setTo(b); }} />
        {tab === 'list' && (
          <>
            <div className="chips">
              <button className={`chip small${by === 'created' ? ' active' : ''}`} onClick={() => setBy('created')}>По дате создания</button>
              <button className={`chip small${by === 'period' ? ' active' : ''}`} onClick={() => setBy('period')}>По периоду оплаты</button>
            </div>
            <StoreChips all stores={s.stores} value={store} onChange={setStore} />
            {list.loading && <Spinner />}
            {list.error && <div className="error">{list.error}</div>}
            {!list.loading && (list.data ?? []).length === 0 && <div className="empty">Ничего не найдено</div>}
            {(list.data ?? []).length > 0 && <div className="total-row"><span>Итого</span><span>{rub(total)}</span></div>}
            {(list.data ?? []).map((p) => (
              <div key={p.id} className="card row">
                <div className="grow">
                  <div className="title">{p.categoryName ?? 'Без категории'}</div>
                  <div className="sub">{fmtDate(p.paidOn)}{p.periodFrom && p.periodTo ? ` · за ${fmtDate(p.periodFrom)}–${fmtDate(p.periodTo)}` : ''} · {(wallets.data ?? []).find((w) => w.id === p.walletId)?.name ?? ''}{p.details ? ` · ${p.details}` : ''}</div>
                </div>
                <span className="price">{rub(p.amount)}</span>
                <button className="icon-btn small" aria-label="Удалить платёж" onClick={() => remove(p.id)}><Icon name="trash" size={18} /></button>
              </div>
            ))}
          </>
        )}
        {tab === 'report' && (
          <>
            {report.loading && <Spinner />}
            {report.error && <div className="error">{report.error}</div>}
            {(report.data?.wallets ?? []).map((w) => (
              <div key={w.walletId} className="card">
                <div className="title">{w.name}</div>
                <div className="total-row"><span>На начало</span><span>{rub(w.opening)}</span></div>
                <div className="total-row"><span>Поступления</span><span>{rub(w.inflow)}</span></div>
                <div className="total-row"><span>Выплаты</span><span>{rub(w.outflow)}</span></div>
                {Object.entries(w.byKind).map(([k, v]) => <div key={k} className="sub">{k}: +{rub(v.in)} / −{rub(v.out)}</div>)}
                <div className="total-row"><b>На конец</b><b>{rub(w.closing)}</b></div>
              </div>
            ))}
          </>
        )}
      </main>
      {form && <PaymentForm onClose={() => setForm(false)} onDone={() => { setForm(false); list.reload(); say('Успешно'); }} wallets={wallets.data ?? []} />}
      {toast}
    </>
  );
}

function PaymentForm({ onClose, onDone, wallets }: { onClose: () => void; onDone: () => void; wallets: Wallet[] }) {
  const s = useSession();
  const cats = useLoad(() => api<ListItem[]>('/lists/spending_categories'), []);
  const [store, setStore] = useState(s.storeId ?? '');
  const [cat, setCat] = useState('');
  const [wallet, setWallet] = useState('');
  const [amount, setAmount] = useState('');
  const [date, setDate] = useState(todayLocal());
  const [pf, setPf] = useState('');
  const [pt, setPt] = useState('');
  const [details, setDetails] = useState('');
  const [error, setError] = useState<string | null>(null);

  async function save() {
    setError(null);
    try {
      await api('/payments', { body: { storeId: store, categoryId: cat || undefined, walletId: wallet, amount: Number(amount.replace(',', '.')), paidOn: date, periodFrom: pf || undefined, periodTo: pt || undefined, details: details || undefined } });
      onDone();
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Ошибка');
    }
  }

  return (
    <Modal onClose={onClose}>
      <div className="h2" style={{ marginTop: 0 }}>Новый платёж</div>
      <StoreChips stores={s.stores} value={store} onChange={setStore} />
      <label className="field"><span>Категория</span>
        <select value={cat} onChange={(e) => setCat(e.target.value)}><option value="">Без категории</option>{(cats.data ?? []).map((c) => <option key={c.id} value={c.id}>{c.name}</option>)}</select>
      </label>
      <label className="field"><span>Счёт</span>
        <select value={wallet} onChange={(e) => setWallet(e.target.value)}><option value="">Выберите счёт</option>{wallets.filter((w) => !w.storeId || w.storeId === store).map((w) => <option key={w.id} value={w.id}>{w.name}</option>)}</select>
      </label>
      <label className="field"><span>Сумма</span><input type="number" min="0" step="any" value={amount} onChange={(e) => setAmount(e.target.value)} /></label>
      <label className="field"><span>Дата платежа</span><input type="date" value={date} onChange={(e) => setDate(e.target.value)} /></label>
      <div className="row">
        <label className="field grow"><span>Период с</span><input type="date" value={pf} onChange={(e) => setPf(e.target.value)} /></label>
        <label className="field grow"><span>по</span><input type="date" value={pt} onChange={(e) => setPt(e.target.value)} /></label>
      </div>
      <label className="field"><span>Детали</span><input type="text" value={details} onChange={(e) => setDetails(e.target.value)} /></label>
      {error && <div className="error" role="alert">{error}</div>}
      <button className="btn" disabled={!wallet || !(Number(amount) > 0)} onClick={save}>Сохранить</button>
    </Modal>
  );
}
