import { useState } from 'react';
import { api, type SalaryDetail, type SalaryRow, type Wallet } from '../api';
import { fmtDate, fmtDateTime, monthStartLocal, todayLocal } from '../dates';
import { Icon } from '../Icon';
import { num, rub } from '../money';
import { Line } from '../reportKit';
import { useSession } from '../session';
import { DateRange, Header, Modal, Spinner, StoreChips, useLoad, useToast } from '../ui';

const TYPES: Record<string, string> = { none: 'Нет', hour: 'За час', day: 'За день / смену', month: 'За месяц' };

/** Зарплата: начислено, выплачено и к выплате по сотрудникам за период; детали, рабочие часы и выплаты. */
export default function Salary() {
  const s = useSession();
  const [from, setFrom] = useState(monthStartLocal());
  const [to, setTo] = useState(todayLocal());
  const [store, setStore] = useState('all');
  const q = { fromDate: from, toDate: to, storeId: store === 'all' ? undefined : store };
  const list = useLoad(() => api<SalaryRow[]>('/salary', { query: q }), [from, to, store]);
  const [open, setOpen] = useState<string | null>(null);

  return (
    <>
      <Header title="Зарплата" />
      <main className="page wide">
        <DateRange from={from} to={to} onChange={(a, b) => { setFrom(a); setTo(b); }} />
        <StoreChips all stores={s.stores} value={store} onChange={setStore} />
        {list.loading && <Spinner />}
        {list.error && <div className="error">{list.error}</div>}
        {!list.loading && (list.data ?? []).length === 0 && <div className="empty">Нет данных за период</div>}
        {(list.data ?? []).length > 0 && (
          <div style={{ overflowX: 'auto' }}>
            <table className="table">
              <thead>
                <tr><th>Сотрудник</th><th>Тип</th><th className="num">Часы / смены</th><th className="num">Бонус</th><th className="num">Начислено</th><th className="num">Выплачено</th><th className="num">К выплате</th></tr>
              </thead>
              <tbody>
                {(list.data ?? []).map((r) => (
                  <tr key={r.userId} style={{ cursor: 'pointer' }} onClick={() => setOpen(r.userId)}>
                    <td>{r.name}<div className="sub">{r.roleName}</div></td>
                    <td>{TYPES[r.salaryType] ?? r.salaryType}{r.salaryType !== 'none' ? ` · ${rub(r.rate)}` : ''}</td>
                    <td className="num">{num(r.hours)} / {r.shifts}</td>
                    <td className="num">{rub(r.bonus)}</td>
                    <td className="num">{rub(r.accrued)}</td>
                    <td className="num">{rub(r.paid)}</td>
                    <td className="num"><b>{rub(r.toPay)}</b></td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </main>
      {open && <Detail userId={open} q={q} onClose={() => { setOpen(null); list.reload(); }} />}
    </>
  );
}

function Detail({ userId, q, onClose }: { userId: string; q: Record<string, string | undefined>; onClose: () => void }) {
  const s = useSession();
  const [toast, say] = useToast();
  const d = useLoad(() => api<SalaryDetail>(`/salary/${userId}`, { query: q }), [userId]);
  const wallets = useLoad(() => api<Wallet[]>('/wallets'), []);
  const [paying, setPaying] = useState(false);
  const [amount, setAmount] = useState('');
  const [wallet, setWallet] = useState('');
  const [error, setError] = useState<string | null>(null);
  const canPay = s.can('salary.view_others');

  async function pay() {
    setError(null);
    try {
      await api(`/salary/${userId}/payments`, { body: { amount: Number(amount.replace(',', '.')), walletId: wallet || undefined } });
      setPaying(false);
      setAmount('');
      d.reload();
      say('Успешно');
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Ошибка');
    }
  }

  const x = d.data;
  return (
    <Modal onClose={onClose}>
      {!x && <Spinner />}
      {x && (
        <>
          <div className="h2" style={{ marginTop: 0 }}>{x.name}</div>
          <Line label="Роль" value={x.roleName} />
          <Line label="Тип зарплаты" value={`${TYPES[x.salaryType] ?? x.salaryType}${x.salaryType !== 'none' ? ` · ${rub(x.rate)}` : ''}`} />
          <Line label="Часы работы в текущем периоде" value={num(x.hours)} />
          <Line label="Начислено за период" value={rub(x.accrued)} />
          <Line label="Выплачено за период" value={rub(x.paid)} />
          <Line bold label="Баланс за период" value={rub(x.balancePeriod)} />
          <Line bold label="Баланс за все время" value={rub(x.balanceAll)} />

          <div className="h2" style={{ fontSize: 18 }}>Часы работы</div>
          {x.workingHours.length === 0 && <div className="muted">Нет данных за период</div>}
          {x.workingHours.map((h) => <Line key={h.id} label={fmtDateTime(h.startedAt)} value={h.endedAt ? fmtDateTime(h.endedAt) : 'идёт'} />)}

          <div className="h2" style={{ fontSize: 18 }}>Выплаты</div>
          {x.payments.length === 0 && <div className="muted">Нет данных за период</div>}
          {x.payments.map((p) => (
            <div key={p.id} className="row" style={{ justifyContent: 'space-between', padding: '6px 0' }}>
              <span>{fmtDate(p.paidOn)}</span>
              <span className="row">
                <b>{rub(p.amount)}</b>
                {canPay && <button className="icon-btn small" aria-label="Удалить выплату" onClick={() => api(`/salary-payments/${p.id}`, { method: 'DELETE' }).then(d.reload).catch(() => undefined)}><Icon name="trash" size={16} /></button>}
              </span>
            </div>
          ))}
          {canPay && !paying && <button className="btn" style={{ marginTop: 16 }} onClick={() => { setPaying(true); setAmount(String(Math.max(x.toPay, 0))); }}>Выплатить</button>}
          {paying && (
            <div style={{ marginTop: 16 }}>
              <label className="field"><span>Сумма</span><input type="number" min="0" step="any" value={amount} onChange={(e) => setAmount(e.target.value)} /></label>
              <label className="field"><span>Денежный счёт</span>
                <select value={wallet} onChange={(e) => setWallet(e.target.value)}>
                  <option value="">Без счёта</option>
                  {(wallets.data ?? []).map((w) => <option key={w.id} value={w.id}>{w.name}</option>)}
                </select>
              </label>
              {error && <div className="error" role="alert">{error}</div>}
              <button className="btn" disabled={!(Number(amount) > 0)} onClick={pay}>Выплатить</button>
            </div>
          )}
        </>
      )}
      {toast}
    </Modal>
  );
}
