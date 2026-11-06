import { useState, type FormEvent } from 'react';
import { useNavigate } from 'react-router-dom';
import { api, type Client, type ListItem } from '../api';
import { Icon } from '../Icon';
import { num, rub } from '../money';
import { ProductPicker } from '../ProductPicker';
import { useSession } from '../session';
import { Header, Modal, Spinner, useLoad } from '../ui';

type Mode = 'registered' | 'search' | 'product' | 'last';
const MODES: { id: Mode; label: string }[] = [
  { id: 'registered', label: 'По дате регистрации' },
  { id: 'search', label: 'По номеру телефона или имени' },
  { id: 'product', label: 'По купленным товарам' },
  { id: 'last', label: 'По последней дате покупки' },
];

// период статистики → число суток
function periodDays(id: string): number {
  const now = new Date();
  if (id === '7') return 7;
  if (id === '30') return 30;
  if (id === 'week') return ((now.getDay() + 6) % 7) + 1; // с понедельника
  return now.getDate(); // с начала месяца
}
const PERIODS = [
  { id: '7', label: 'Последние 7 дней' },
  { id: '30', label: 'Последние 30 дней' },
  { id: 'week', label: 'Эта неделя' },
  { id: 'month', label: 'Этот месяц' },
];

export default function Clients() {
  const s = useSession();
  const nav = useNavigate();
  const [period, setPeriod] = useState('7');
  const [mode, setMode] = useState<Mode>('registered');
  const [search, setSearch] = useState('');
  const [productId, setProductId] = useState<string | null>(null);
  const [productName, setProductName] = useState('');
  const [picker, setPicker] = useState(false);
  const [creating, setCreating] = useState(false);

  const stats = useLoad(() => api<{ total: number; new: number; active: number }>('/clients/stats', { query: { days: periodDays(period) } }), [period]);
  const list = useLoad(
    () => api<Client[]>('/clients', { query: { search: mode === 'search' ? search : undefined, productId: mode === 'product' ? productId : undefined, sort: mode === 'registered' ? 'registered' : mode === 'last' ? 'last_purchase' : undefined } }),
    [mode, search, productId],
  );

  return (
    <>
      <Header title="Клиенты" right={<button className="icon-btn" aria-label="Новый клиент" onClick={() => setCreating(true)}><Icon name="plus" /></button>} />
      <main className="page">
        <div className="h2" style={{ marginTop: 0, fontSize: 18 }}>Статистика клиентов</div>
        <div className="chips">
          {PERIODS.map((p) => <button key={p.id} className={`chip small${period === p.id ? ' active' : ''}`} onClick={() => setPeriod(p.id)}>{p.label}</button>)}
        </div>
        <div className="row wrap" style={{ marginBottom: 20 }}>
          {[['Всего клиентов', stats.data?.total], ['Новые клиенты', stats.data?.new], ['Активные клиенты', stats.data?.active]].map(([label, v]) => (
            <div key={String(label)} className="card grow" style={{ minWidth: 150, padding: 16 }}>
              <div className="sub" style={{ marginTop: 0 }}>{label}</div>
              <div style={{ fontSize: 20, fontWeight: 700 }}>{v ?? '—'}</div>
            </div>
          ))}
        </div>

        <div className="h2" style={{ marginTop: 0, fontSize: 18 }}>Поиск клиентов</div>
        <div className="chips">
          {MODES.map((m) => <button key={m.id} className={`chip small${mode === m.id ? ' active' : ''}`} onClick={() => setMode(m.id)}>{m.label}</button>)}
        </div>
        {mode === 'search' && <input type="search" placeholder="Имя или телефон" value={search} onChange={(e) => setSearch(e.target.value)} style={{ marginBottom: 14 }} />}
        {mode === 'product' && (
          <div className="card click row" onClick={() => setPicker(true)}>
            <div className="grow title">{productName || 'Выбрать товар'}</div><span className="link">{productId ? 'Изменить' : ''}</span>
          </div>
        )}
        <button className="link" onClick={list.reload}>Обновить</button>
        {list.loading && <Spinner />}
        {list.error && <div className="error">{list.error}</div>}
        {!list.loading && (list.data ?? []).length === 0 && <div className="empty">Ничего не найдено</div>}
        {(list.data ?? []).map((c) => (
          <div key={c.id} className="card click row" onClick={() => s.can('client.card') && nav(`/client/${c.id}`)}>
            <div className="grow"><div className="title">{c.name}</div><div className="sub">{c.phone || 'без телефона'}</div></div>
            <div style={{ textAlign: 'right' }}>
              <div className="sub" style={{ marginTop: 0 }}>{num(c.purchases)} покупок / {rub(c.purchasesSum)}</div>
              <div className="sub" style={{ marginTop: 2 }}>бонусы {rub(c.bonusBalance)}</div>
            </div>
          </div>
        ))}
      </main>
      {picker && s.storeId && <ProductPicker storeId={s.storeId} onClose={() => setPicker(false)} onPick={(p) => { setProductId(p.id); setProductName(p.name); setPicker(false); }} />}
      {creating && <NewClient onClose={() => setCreating(false)} onCreated={(id) => nav(`/client/${id}`)} />}
    </>
  );
}

function NewClient({ onClose, onCreated }: { onClose: () => void; onCreated: (id: string) => void }) {
  const s = useSession();
  const discounts = useLoad(() => api<ListItem[]>('/lists/discounts'), []);
  const [f, setF] = useState({ name: '', phone: '+', address: '', clientType: 'person', discountId: '' });
  const [error, setError] = useState<string | null>(null);

  async function submit(e: FormEvent) {
    e.preventDefault();
    setError(null);
    try {
      const c = await api<Client>('/clients', { body: { name: f.name, phone: f.phone.trim() === '+' ? '' : f.phone, address: f.address, clientType: f.clientType, discountId: f.discountId || undefined } });
      if (s.can('client.card')) onCreated(c.id);
      else onClose();
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Ошибка');
    }
  }

  return (
    <Modal onClose={onClose}>
      <form onSubmit={submit}>
        <div className="h2" style={{ marginTop: 0 }}>Новый клиент</div>
        <div className="chips">
          <button type="button" className={`chip${f.clientType === 'person' ? ' active' : ''}`} onClick={() => setF({ ...f, clientType: 'person' })}>Физ. лицо</button>
          <button type="button" className={`chip${f.clientType === 'organization' ? ' active' : ''}`} onClick={() => setF({ ...f, clientType: 'organization' })}>Организация</button>
        </div>
        <label className="field"><span>Имя</span><input type="text" value={f.name} onChange={(e) => setF({ ...f, name: e.target.value })} /></label>
        <label className="field"><span>Телефон</span><input type="text" inputMode="tel" value={f.phone} placeholder="+xxxxxxxxxxxx" onChange={(e) => setF({ ...f, phone: e.target.value })} /></label>
        <label className="field"><span>Адрес</span><input type="text" value={f.address} onChange={(e) => setF({ ...f, address: e.target.value })} /></label>
        <label className="field"><span>Постоянная скидка</span>
          <select value={f.discountId} onChange={(e) => setF({ ...f, discountId: e.target.value })}>
            <option value="">Без скидки</option>
            {(discounts.data ?? []).map((d) => <option key={d.id} value={d.id}>{d.name}</option>)}
          </select>
        </label>
        {error && <div className="error" role="alert">{error}</div>}
        <button className="btn" disabled={!f.name.trim()}>Создать</button>
      </form>
    </Modal>
  );
}
