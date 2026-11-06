import { useEffect, useState } from 'react';
import { useNavigate, useParams } from 'react-router-dom';
import { api, type Client, type ListItem, type Purchase } from '../api';
import { fmtDateTime } from '../dates';
import { Icon } from '../Icon';
import { num, rub } from '../money';
import { useSession } from '../session';
import { Header, Modal, Spinner, useLoad, useToast } from '../ui';

interface Loyalty { cashbackEnabled: boolean; punchCardEnabled: boolean; defaultBonusLevel: number; punchCardCapacity: number }

export default function ClientDetail() {
  const { id = '' } = useParams();
  const s = useSession();
  const nav = useNavigate();
  const [toast, say] = useToast();
  const client = useLoad(() => api<Client>(`/clients/${id}`), [id]);
  const discounts = useLoad(() => api<ListItem[]>('/lists/discounts'), []);
  const loyalty = useLoad(() => api<Loyalty>('/loyalty-settings'), []);
  const history = useLoad(() => api<Purchase[]>(`/clients/${id}/purchase-history`), [id]);
  const [f, setF] = useState({ name: '', phone: '', address: '', clientType: 'person', discountId: '', bonusLevel: '', comment: '' });
  const [error, setError] = useState<string | null>(null);
  const [bonus, setBonus] = useState<null | 'add' | 'sub'>(null);

  useEffect(() => {
    const c = client.data;
    if (c) setF({ name: c.name, phone: c.phone, address: c.address, clientType: c.clientType, discountId: c.discountId ?? '', bonusLevel: c.bonusLevel === null ? '' : String(c.bonusLevel), comment: c.comment });
  }, [client.data]);

  if (client.loading && !client.data) return <><Header title="Клиент" /><Spinner /></>;
  const c = client.data;
  if (!c) return <><Header title="Клиент" /><div className="empty">{client.error ?? 'Клиент не найден'}</div></>;
  const avg = c.purchases > 0 ? c.purchasesSum / c.purchases : 0;

  async function save() {
    setError(null);
    try {
      await api(`/clients/${id}`, {
        method: 'PUT',
        body: { name: f.name, phone: f.phone, address: f.address, clientType: f.clientType, discountId: f.discountId || null, comment: f.comment, bonusLevel: f.bonusLevel === '' ? undefined : Number(f.bonusLevel.replace(',', '.')) },
      });
      say('Успешно');
      client.reload();
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Ошибка');
    }
  }

  async function remove() {
    if (!confirm('Удалить клиента?')) return;
    await api(`/clients/${id}`, { method: 'DELETE' }).then(() => nav('/clients', { replace: true })).catch((e: unknown) => say(e instanceof Error ? e.message : 'Ошибка'));
  }

  async function resetPunch() {
    await api(`/clients/${id}/punch-card/reset`, { method: 'POST' });
    client.reload();
  }

  return (
    <>
      <Header title={c.name} right={s.can('client.delete') ? <button className="icon-btn" aria-label="Удалить клиента" onClick={remove}><Icon name="trash" /></button> : undefined} />
      <main className="page">
        <div className="chips">
          <button className={`chip${f.clientType === 'person' ? ' active' : ''}`} onClick={() => setF({ ...f, clientType: 'person' })}>Физ. лицо</button>
          <button className={`chip${f.clientType === 'organization' ? ' active' : ''}`} onClick={() => setF({ ...f, clientType: 'organization' })}>Организация</button>
        </div>
        <label className="field"><span>Имя</span><input type="text" value={f.name} onChange={(e) => setF({ ...f, name: e.target.value })} /></label>
        <label className="field"><span>Телефон</span><input type="text" value={f.phone} placeholder="+xxxxxxxxxxxx" onChange={(e) => setF({ ...f, phone: e.target.value })} /></label>
        <label className="field"><span>Адрес</span><input type="text" value={f.address} onChange={(e) => setF({ ...f, address: e.target.value })} /></label>
        <label className="field"><span>Постоянная скидка</span>
          <select value={f.discountId} onChange={(e) => setF({ ...f, discountId: e.target.value })}>
            <option value="">Без скидки</option>
            {(discounts.data ?? []).map((d) => <option key={d.id} value={d.id}>{d.name}</option>)}
          </select>
        </label>
        {loyalty.data?.cashbackEnabled && (
          <label className="field"><span>Уровень бонусов, %</span>
            <input type="number" min="0" max="100" step="any" value={f.bonusLevel} placeholder={String(loyalty.data.defaultBonusLevel)} onChange={(e) => setF({ ...f, bonusLevel: e.target.value })} />
          </label>
        )}
        <label className="field"><span>Комментарий</span><input type="text" value={f.comment} onChange={(e) => setF({ ...f, comment: e.target.value })} /></label>
        {error && <div className="error" role="alert">{error}</div>}
        <button className="btn" onClick={save}>Сохранить</button>

        {loyalty.data?.cashbackEnabled && (
          <>
            <div className="h2">Бонусы</div>
            <div className="card">
              <div className="row" style={{ justifyContent: 'space-between' }}><span>Баланс</span><b>{rub(c.bonusBalance)}</b></div>
            </div>
            <div className="row">
              <button className="btn light" onClick={() => setBonus('add')}>Начислить</button>
              <button className="btn light" onClick={() => setBonus('sub')}>Списать</button>
            </div>
          </>
        )}
        {loyalty.data?.punchCardEnabled && (
          <>
            <div className="h2">Карта «N-й товар в подарок»</div>
            <div className="card row">
              <div className="grow">Отметок: {c.punches} из {loyalty.data.punchCardCapacity}</div>
              <button className="link" onClick={resetPunch}>Сбросить</button>
            </div>
          </>
        )}

        <div className="h2">Статистика</div>
        <div className="card">
          <div className="row" style={{ justifyContent: 'space-between' }}><span>Покупок</span><b>{num(c.purchases)}</b></div>
          <div className="row" style={{ justifyContent: 'space-between', marginTop: 6 }}><span>Сумма</span><b>{rub(c.purchasesSum)}</b></div>
          <div className="row" style={{ justifyContent: 'space-between', marginTop: 6 }}><span>Средний чек</span><b>{rub(avg)}</b></div>
          <div className="row" style={{ justifyContent: 'space-between', marginTop: 6 }}><span>Последняя покупка</span><b>{c.lastPurchaseAt ? fmtDateTime(c.lastPurchaseAt) : '—'}</b></div>
        </div>

        <div className="h2">История покупок</div>
        {(history.data ?? []).length === 0 && <div className="empty">Покупок пока нет</div>}
        {(history.data ?? []).map((p) => (
          <div key={p.id} className="card click row" onClick={() => s.can('sale.history') && nav(`/sale/${p.id}`)}>
            <div className="grow"><div className="title">{fmtDateTime(p.saleDate)}</div><div className="sub">{s.stores.find((x) => x.id === p.storeId)?.name}</div></div>
            <span className="price">{rub(p.total)}</span>
          </div>
        ))}
      </main>
      {bonus && <BonusModal sign={bonus === 'add' ? 1 : -1} clientId={id} onClose={() => setBonus(null)} onDone={() => { setBonus(null); client.reload(); say('Успешно'); }} />}
      {toast}
    </>
  );
}

/** «Начислить / Списать»: сумма не равна нулю, комментарий; повторная отправка безопасна (operationId). */
function BonusModal({ sign, clientId, onClose, onDone }: { sign: 1 | -1; clientId: string; onClose: () => void; onDone: () => void }) {
  const [amount, setAmount] = useState('');
  const [comment, setComment] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [opId] = useState(() => crypto.randomUUID());

  async function submit() {
    setError(null);
    try {
      await api(`/clients/${clientId}/bonus-adjustments`, { body: { amount: sign * Math.abs(Number(amount.replace(',', '.'))), comment, operationId: opId } });
      onDone();
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Ошибка');
    }
  }

  return (
    <Modal onClose={onClose}>
      <div className="h2" style={{ marginTop: 0 }}>{sign > 0 ? 'Начислить бонусы' : 'Списать бонусы'}</div>
      <label className="field"><span>Сумма</span><input type="number" min="0" step="any" value={amount} onChange={(e) => setAmount(e.target.value)} /></label>
      <label className="field"><span>Причина / комментарий</span><input type="text" value={comment} onChange={(e) => setComment(e.target.value)} /></label>
      {error && <div className="error" role="alert">{error}</div>}
      <button className="btn" disabled={!(Number(amount) > 0)} onClick={submit}>Сохранить</button>
    </Modal>
  );
}
