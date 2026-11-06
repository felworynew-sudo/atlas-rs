import { useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { api, type Spending, type Wallet } from '../api';
import { fmtDate, todayLocal } from '../dates';
import { Icon } from '../Icon';
import { rub } from '../money';
import { useSession } from '../session';
import { DateRange, Header, Spinner, StoreChips, useLoad, useToast } from '../ui';

export default function Spendings() {
  const s = useSession();
  const nav = useNavigate();
  const [toast, say] = useToast();
  const [from, setFrom] = useState(todayLocal());
  const [to, setTo] = useState(todayLocal());
  const [store, setStore] = useState('all');
  const list = useLoad(() => api<Spending[]>('/spendings', { query: { from, to, storeId: store === 'all' ? undefined : store } }), [from, to, store]);
  const wallets = useLoad(() => api<Wallet[]>('/wallets'), []);
  const total = (list.data ?? []).reduce((sum, x) => sum + x.amount, 0);

  async function remove(id: string) {
    if (!confirm('Удалить расход? Деньги вернутся на счёт.')) return;
    try {
      await api(`/spendings/${id}`, { method: 'DELETE' });
      list.reload();
    } catch (e) {
      say(e instanceof Error ? e.message : 'Ошибка');
    }
  }

  return (
    <>
      <Header title="Расходы" right={<button className="icon-btn" aria-label="Новый расход" onClick={() => nav('/spending/new')}><Icon name="plus" /></button>} />
      <main className="page">
        <DateRange from={from} to={to} onChange={(a, b) => { setFrom(a); setTo(b); }} />
        <StoreChips all stores={s.stores} value={store} onChange={setStore} />
        <button className="link" onClick={list.reload}>Обновить</button>
        {list.loading && <Spinner />}
        {list.error && <div className="error">{list.error}</div>}
        {!list.loading && (list.data ?? []).length === 0 && <div className="empty">Ничего не найдено</div>}
        {(list.data ?? []).length > 0 && <div className="total-row"><span>Итого</span><span>{rub(total)}</span></div>}
        {(list.data ?? []).map((x) => (
          <div key={x.id} className="card row">
            <div className="grow">
              <div className="title">{x.categoryName ?? 'Без категории'}{x.kind === 'capital' ? ' · капитальный' : ''}</div>
              <div className="sub">{fmtDate(x.spendingDate)} · {x.details || '—'} · {(wallets.data ?? []).find((w) => w.id === x.walletId)?.name ?? 'без счёта'}</div>
            </div>
            <span className="price">{rub(x.amount)}</span>
            {s.can('spending.delete') && <button className="icon-btn small" aria-label="Удалить расход" onClick={() => remove(x.id)}><Icon name="trash" size={18} /></button>}
          </div>
        ))}
      </main>
      {toast}
    </>
  );
}
