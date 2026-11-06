import { useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { api, type ListItem, type Wallet } from '../api';
import { todayLocal } from '../dates';
import { useSession } from '../session';
import { Header, StoreChips, useLoad, useToast } from '../ui';

/** Новый расход: категория, детали, сумма, источник (денежный счёт), дата, тип (операционный или капитальный). */
export default function SpendingForm() {
  const s = useSession();
  const nav = useNavigate();
  const [toast, say] = useToast();
  const cats = useLoad(() => api<ListItem[]>('/lists/spending_categories'), []);
  const wallets = useLoad(() => api<Wallet[]>('/wallets'), []);
  const [store, setStore] = useState(s.storeId ?? '');
  const [category, setCategory] = useState('');
  const [details, setDetails] = useState('');
  const [amount, setAmount] = useState('');
  const [wallet, setWallet] = useState('');
  const [date, setDate] = useState(todayLocal());
  const [kind, setKind] = useState<'operating' | 'capital'>('operating');
  const [error, setError] = useState<string | null>(null);
  const storeWallets = (wallets.data ?? []).filter((w) => !w.storeId || w.storeId === store);

  async function save() {
    setError(null);
    try {
      await api('/spendings', {
        body: {
          storeId: store,
          categoryId: category || undefined,
          details: details || undefined,
          amount: Number(amount.replace(',', '.')),
          walletId: wallet || undefined,
          spendingDate: date,
          kind,
        },
      });
      say('Успешно');
      setTimeout(() => nav('/spendings', { replace: true }), 700);
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Ошибка');
    }
  }

  return (
    <>
      <Header title="Новый расход" />
      <main className="page">
        <StoreChips stores={s.stores} value={store} onChange={setStore} />
        <label className="field"><span>Категория</span>
          <select value={category} onChange={(e) => setCategory(e.target.value)}>
            <option value="">Выберите категорию</option>
            {(cats.data ?? []).map((c) => <option key={c.id} value={c.id}>{c.name}</option>)}
          </select>
        </label>
        <label className="field"><span>Детали</span><input type="text" value={details} onChange={(e) => setDetails(e.target.value)} /></label>
        <label className="field"><span>Сумма</span><input type="number" min="0" step="any" value={amount} placeholder="0" onChange={(e) => setAmount(e.target.value)} /></label>
        <label className="field"><span>Источник</span>
          <select value={wallet} onChange={(e) => setWallet(e.target.value)}>
            <option value="">Без денежного счёта</option>
            {storeWallets.map((w) => <option key={w.id} value={w.id}>{w.name}</option>)}
          </select>
        </label>
        <label className="field"><span>Дата</span><input type="date" value={date} onChange={(e) => setDate(e.target.value)} /></label>
        <span className="label">Тип</span>
        <div className="chips">
          <button className={`chip${kind === 'operating' ? ' active' : ''}`} onClick={() => setKind('operating')}>Операционные расходы (повседневные)</button>
          <button className={`chip${kind === 'capital' ? ' active' : ''}`} onClick={() => setKind('capital')}>Капитальные вложения (большие и необычные — ремонт, покупка оборудования)</button>
        </div>
        {error && <div className="error" role="alert">{error}</div>}
        <button className="btn" disabled={!category || !(Number(amount) > 0)} onClick={save}>Сохранить</button>
      </main>
      {toast}
    </>
  );
}
