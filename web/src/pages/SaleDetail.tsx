import { useState } from 'react';
import { useNavigate, useParams } from 'react-router-dom';
import { api, type ListItem, type Sale } from '../api';
import { num, rub } from '../money';
import { useSession } from '../session';
import { Header, Modal, Spinner, useLoad, useToast } from '../ui';

export default function SaleDetail() {
  const { id = '' } = useParams();
  const s = useSession();
  const nav = useNavigate();
  const [toast, say] = useToast();
  const sale = useLoad(() => api<Sale>(`/sales/${id}`), [id]);
  const reasons = useLoad(() => api<ListItem[]>('/lists/return_reasons'), []);
  const [returning, setReturning] = useState(false);
  const [qty, setQty] = useState<Record<string, string>>({});
  const [kind, setKind] = useState<'money' | 'exchange'>('money');
  const [restock, setRestock] = useState(true);
  const [reason, setReason] = useState('');
  const [error, setError] = useState<string | null>(null);

  if (sale.loading) return <><Header title="Продажа" /><Spinner /></>;
  const d = sale.data;
  if (!d) return <><Header title="Продажа" /><div className="empty">{sale.error ?? 'Продажа не найдена'}</div></>;

  async function makeReturn() {
    setError(null);
    const items = Object.entries(qty).filter(([, v]) => Number(v) > 0).map(([saleItemId, v]) => ({ saleItemId, qty: Number(v) }));
    try {
      const r = await api<{ amount: number }>(`/sales/${id}/returns`, { body: { items, returnType: kind, restock, reasonId: reason || undefined } });
      setReturning(false);
      setQty({});
      sale.reload();
      say(`Успешно: возврат ${rub(r.amount)}`);
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Ошибка');
    }
  }

  async function remove() {
    if (!confirm('Удалить продажу полностью? Остатки вернутся на склад.')) return;
    try {
      await api(`/sales/${id}`, { method: 'DELETE' });
      nav(-1);
    } catch (e) {
      say(e instanceof Error ? e.message : 'Ошибка');
    }
  }

  const when = new Date(d.saleDate).toLocaleString('ru-RU', { dateStyle: 'long', timeStyle: 'short' });
  const canReturn = d.items.some((i) => i.qty - i.returnedQty > 0);

  return (
    <>
      <Header title="Продажа" />
      <main className="page">
        <div className="muted" style={{ textAlign: 'center', fontSize: 12 }}>{d.id}</div>
        <div className="card" style={{ marginTop: 14 }}>
          <div className="title">{rub(d.total)}</div>
          <div className="sub">{d.userName} · {s.stores.find((x) => x.id === d.storeId)?.name} · {when}</div>
        </div>
        <table className="table">
          <thead><tr><th>Название</th><th className="num">Кол-во</th><th className="num">Сумма</th></tr></thead>
          <tbody>
            {d.items.map((i) => (
              <tr key={i.id}>
                <td>{i.name}{i.returnedQty > 0 && <div className="low" style={{ fontSize: 12 }}>возвращено {num(i.returnedQty)}</div>}</td>
                <td className="num">{num(i.qty)}</td>
                <td className="num">{rub(i.price * i.qty)}</td>
              </tr>
            ))}
            {d.discountAmount > 0 && <tr><td colSpan={2}>Скидка</td><td className="num">−{rub(d.discountAmount)}</td></tr>}
            <tr><td colSpan={2}><b>Итого</b></td><td className="num"><b>{rub(d.total)}</b></td></tr>
          </tbody>
        </table>
        {d.returnedAmount > 0 && <div className="low" style={{ margin: '12px 0' }}>Возвращено {rub(d.returnedAmount)}</div>}

        <div style={{ marginTop: 24, display: 'grid', gap: 10 }}>
          {canReturn && s.can('sale.history') && <button className="btn danger" onClick={() => setReturning(true)}>Оформить возврат</button>}
          {s.can('sale.delete') && <button className="btn danger" onClick={remove}>Удалить полностью продажу</button>}
        </div>
      </main>

      {returning && (
        <Modal onClose={() => setReturning(false)}>
          <div className="h2" style={{ marginTop: 0 }}>Возврат</div>
          {d.items.filter((i) => i.qty - i.returnedQty > 0).map((i) => (
            <label key={i.id} className="field">
              <span>{i.name} (можно вернуть {num(i.qty - i.returnedQty)})</span>
              <input type="number" min="0" max={i.qty - i.returnedQty} value={qty[i.id] ?? ''} onChange={(e) => setQty({ ...qty, [i.id]: e.target.value })} />
            </label>
          ))}
          <div className="chips">
            <button className={`chip${kind === 'money' ? ' active' : ''}`} onClick={() => setKind('money')}>Возврат денег</button>
            <button className={`chip${kind === 'exchange' ? ' active' : ''}`} onClick={() => setKind('exchange')}>Замена продукта</button>
          </div>
          <label className="row" style={{ marginBottom: 16 }}>
            <input type="checkbox" checked={restock} onChange={(e) => setRestock(e.target.checked)} style={{ width: 22, height: 22 }} />
            <span>Добавить обратно на склад</span>
          </label>
          <label className="field">
            <span>Причина возврата</span>
            <select value={reason} onChange={(e) => setReason(e.target.value)}>
              <option value="">Не указана</option>
              {(reasons.data ?? []).map((r) => <option key={r.id} value={r.id}>{r.name}</option>)}
            </select>
          </label>
          {error && <div className="error" role="alert">{error}</div>}
          <button className="btn" disabled={!Object.values(qty).some((v) => Number(v) > 0)} onClick={makeReturn}>Сделать возврат</button>
        </Modal>
      )}
      {toast}
    </>
  );
}
