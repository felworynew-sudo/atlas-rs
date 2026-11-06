import { useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { api, type ComplexSale, type ComplexTemplate } from '../api';
import { COMPLEX_STATUSES, daysAgoLocal, endOfDay, fmtDate, startOfDay, todayLocal } from '../dates';
import { Icon } from '../Icon';
import { rub } from '../money';
import { useSession } from '../session';
import { DateRange, Header, Modal, Spinner, StoreChips, useLoad, useToast } from '../ui';

/** Список комплексных продаж (заказы под ключ с расходами и частичной оплатой) + шаблоны. */
export default function ComplexSales() {
  const s = useSession();
  const nav = useNavigate();
  const [toast, say] = useToast();
  const [store, setStore] = useState('all');
  const [from, setFrom] = useState(daysAgoLocal(30));
  const [to, setTo] = useState(todayLocal());
  const [activeOnly, setActiveOnly] = useState(true);
  const [tpl, setTpl] = useState(false);
  const list = useLoad(
    () => api<ComplexSale[]>('/complex-sales', { query: { storeId: store === 'all' ? undefined : store, from: startOfDay(from), to: endOfDay(to), activeOnly: activeOnly ? 'true' : undefined } }),
    [store, from, to, activeOnly],
  );
  const data = list.data ?? [];

  return (
    <>
      <Header
        title="Комплексные продажи"
        right={
          <>
            {s.can('complex_sale.templates') && <button className="link" onClick={() => setTpl(true)}>Шаблоны</button>}
            <button className="icon-btn" aria-label="Новая комплексная продажа" onClick={() => nav('/complex-sale/new')}><Icon name="plus" /></button>
          </>
        }
      />
      <main className="page">
        <DateRange from={from} to={to} onChange={(a, b) => { setFrom(a); setTo(b); }} />
        <StoreChips all stores={s.stores} value={store} onChange={setStore} />
        <label className="row" style={{ marginBottom: 12, gap: 8 }}>
          <input type="checkbox" checked={activeOnly} onChange={(e) => setActiveOnly(e.target.checked)} style={{ width: 20, height: 20 }} />
          <span>Только активные</span>
        </label>
        {list.loading && <Spinner />}
        {list.error && <div className="error">{list.error}</div>}
        {!list.loading && data.length === 0 && <div className="empty">Ничего не найдено</div>}
        {data.map((x) => (
          <div key={x.id} className="card click" onClick={() => nav(`/complex-sale/${x.id}`)}>
            <div className="row">
              <div className="grow">
                <div className="title">{x.name}</div>
                <div className="sub">{fmtDate(x.saleDate)} · {COMPLEX_STATUSES[x.status] ?? x.status}{x.templateName ? ` · ${x.templateName}` : ''}</div>
              </div>
              <span className="price">{rub(x.finalPrice)}</span>
            </div>
            <div className="sub">Оплачено {rub(x.paid)} · Осталось {rub(x.remaining)} · Прибыль {rub(x.expectedProfit)}</div>
          </div>
        ))}
      </main>
      {tpl && <Templates onClose={() => setTpl(false)} say={say} />}
      {toast}
    </>
  );
}

interface TplDraft { id?: string; name: string; finalPrice: string; items: string }

/** Шаблоны: название, цена и состав в формате «название;количество;себестоимость» построчно. */
function Templates({ onClose, say }: { onClose: () => void; say: (t: string) => void }) {
  const list = useLoad(() => api<ComplexTemplate[]>('/complex-sale-templates'), []);
  const [draft, setDraft] = useState<TplDraft | null>(null);
  const [error, setError] = useState<string | null>(null);

  const edit = (t?: ComplexTemplate) =>
    setDraft(t ? { id: t.id, name: t.name, finalPrice: String(t.finalPrice), items: t.items.map((i) => `${i.name};${i.qty ?? 1};${i.cost ?? 0}`).join('\n') } : { name: '', finalPrice: '', items: '' });

  async function save() {
    if (!draft) return;
    setError(null);
    const items = draft.items.split('\n').map((l) => l.trim()).filter(Boolean).map((l) => {
      const [name, qty, cost] = l.split(';').map((x) => x.trim());
      return { name, qty: Number((qty ?? '1').replace(',', '.')) || 1, cost: Number((cost ?? '0').replace(',', '.')) || 0 };
    });
    try {
      await api(draft.id ? `/complex-sale-templates/${draft.id}` : '/complex-sale-templates', { method: draft.id ? 'PUT' : 'POST', body: { name: draft.name, finalPrice: Number(draft.finalPrice.replace(',', '.')) || 0, items } });
      setDraft(null);
      list.reload();
      say('Успешно');
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Ошибка');
    }
  }

  async function remove(id: string) {
    if (!confirm('Удалить шаблон?')) return;
    try {
      await api(`/complex-sale-templates/${id}`, { method: 'DELETE' });
      list.reload();
    } catch (e) {
      say(e instanceof Error ? e.message : 'Ошибка');
    }
  }

  return (
    <Modal onClose={onClose}>
      <div className="h2" style={{ marginTop: 0 }}>Шаблоны комплексных продаж</div>
      {!draft && (
        <>
          {list.loading && <Spinner />}
          {(list.data ?? []).map((t) => (
            <div key={t.id} className="card row">
              <div className="grow click" onClick={() => edit(t)}><div className="title">{t.name}</div><div className="sub">{rub(t.finalPrice)} · позиций {t.items.length}</div></div>
              <button className="icon-btn small" aria-label="Удалить шаблон" onClick={() => remove(t.id)}><Icon name="trash" size={18} /></button>
            </div>
          ))}
          <button className="btn light" onClick={() => edit()}>+ Новый шаблон</button>
        </>
      )}
      {draft && (
        <>
          <label className="field"><span>Название</span><input type="text" value={draft.name} onChange={(e) => setDraft({ ...draft, name: e.target.value })} /></label>
          <label className="field"><span>Стоимость для клиента</span><input type="number" min="0" step="any" value={draft.finalPrice} onChange={(e) => setDraft({ ...draft, finalPrice: e.target.value })} /></label>
          <label className="field"><span>Расходы: название;количество;себестоимость (по строке)</span><textarea rows={5} value={draft.items} onChange={(e) => setDraft({ ...draft, items: e.target.value })} /></label>
          {error && <div className="error" role="alert">{error}</div>}
          <div className="row">
            <button className="btn light" onClick={() => setDraft(null)}>Назад</button>
            <button className="btn" disabled={!draft.name.trim()} onClick={save}>Сохранить</button>
          </div>
        </>
      )}
    </Modal>
  );
}
