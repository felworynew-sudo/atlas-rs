import { useEffect, useState } from 'react';
import { api, type ListItem, type PriceType, type Product } from '../api';
import { Icon } from '../Icon';
import { num } from '../money';
import { useSession } from '../session';
import { Header, Modal, Spinner, StoreChips, useLoad, useToast } from '../ui';

interface Draft {
  id?: string;
  name: string;
  scope: 'store' | 'common';
  autoCalc: boolean;
  markup: string;
  minQty: string;
  minTotal: string;
  paymentMethodId: string;
  byClient: boolean;
}
const empty: Draft = { name: '', scope: 'store', autoCalc: true, markup: '0', minQty: '', minTotal: '', paymentMethodId: '', byClient: false };
const n = (v: string): number => Number(v.replace(',', '.'));

/** Ценовые категории: оптовые, по акции и т.п. Авто-расчёт от базовой цены или ручные цены; правила авто-выбора на кассе. */
export default function PriceTypes() {
  const [toast, say] = useToast();
  const list = useLoad(() => api<PriceType[]>('/price-types'), []);
  const methods = useLoad(() => api<ListItem[]>('/lists/payment_methods'), []);
  const [draft, setDraft] = useState<Draft | null>(null);
  const [prices, setPrices] = useState<PriceType | null>(null);
  const [error, setError] = useState<string | null>(null);

  const edit = (t?: PriceType) =>
    setDraft(t ? {
      id: t.id, name: t.name, scope: t.scope, autoCalc: t.autoCalc, markup: String(t.markupPercent),
      minQty: t.rules.minQty != null ? String(t.rules.minQty) : '', minTotal: t.rules.minTotal != null ? String(t.rules.minTotal) : '',
      paymentMethodId: t.rules.paymentMethodId ?? '', byClient: t.rules.byClient === true,
    } : empty);

  async function save() {
    if (!draft) return;
    setError(null);
    const body = {
      name: draft.name, scope: draft.scope, autoCalc: draft.autoCalc, markupPercent: draft.autoCalc ? n(draft.markup) || 0 : 0,
      rules: { minQty: draft.minQty === '' ? null : n(draft.minQty), minTotal: draft.minTotal === '' ? null : n(draft.minTotal), paymentMethodId: draft.paymentMethodId || null, byClient: draft.byClient },
    };
    try {
      await api(draft.id ? `/price-types/${draft.id}` : '/price-types', { method: draft.id ? 'PUT' : 'POST', body });
      setDraft(null);
      list.reload();
      say('Успешно');
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Ошибка');
    }
  }

  async function remove(id: string) {
    if (!confirm('Удалить тип цены?')) return;
    try {
      await api(`/price-types/${id}`, { method: 'DELETE' });
      setDraft(null);
      list.reload();
    } catch (e) {
      say(e instanceof Error ? e.message : 'Ошибка');
    }
  }

  const ruleText = (t: PriceType): string => {
    const r = t.rules;
    const parts = [r.minQty != null ? `от ${num(r.minQty)} шт` : '', r.minTotal != null ? `чек от ${num(r.minTotal)}` : '', r.paymentMethodId ? `оплата: ${(methods.data ?? []).find((m) => m.id === r.paymentMethodId)?.name ?? '—'}` : '', r.byClient ? 'по клиенту' : ''];
    return parts.filter(Boolean).join(', ') || 'выбирается вручную';
  };

  return (
    <>
      <Header title="Ценовые категории" right={<button className="icon-btn" aria-label="Новый тип цены" onClick={() => edit()}><Icon name="plus" /></button>} />
      <main className="page">
        {list.loading && <Spinner />}
        {!list.loading && (list.data ?? []).length === 0 && <div className="empty">Нет типов цен. Создайте первый тип цены для начала работы</div>}
        {(list.data ?? []).map((t) => (
          <div key={t.id} className="card">
            <div className="row click" onClick={() => edit(t)}>
              <div className="grow">
                <div className="title">{t.name}</div>
                <div className="sub">{t.autoCalc ? `Авто: ${t.markupPercent > 0 ? '+' : ''}${num(t.markupPercent)}% к базовой` : 'Ручные цены'} · {t.scope === 'store' ? 'по филиалам' : 'общие'} · {ruleText(t)}</div>
              </div>
            </div>
            <button className="link" onClick={() => setPrices(t)}>Цены товаров</button>
          </div>
        ))}
      </main>
      {draft && (
        <Modal onClose={() => setDraft(null)}>
          <div className="h2" style={{ marginTop: 0 }}>{draft.id ? 'Тип цены' : 'Новый тип цены'}</div>
          <label className="field"><span>Название типа цены</span><input type="text" value={draft.name} placeholder="Оптовая" onChange={(e) => setDraft({ ...draft, name: e.target.value })} /></label>
          <span className="label">Область действия</span>
          <div className="chips">
            <button className={`chip${draft.scope === 'store' ? ' active' : ''}`} onClick={() => setDraft({ ...draft, scope: 'store' })}>Отдельные цены по филиалам</button>
            <button className={`chip${draft.scope === 'common' ? ' active' : ''}`} onClick={() => setDraft({ ...draft, scope: 'common' })}>Общие цены</button>
          </div>
          <label className="row" style={{ gap: 8, marginBottom: 12 }}>
            <input type="checkbox" checked={draft.autoCalc} style={{ width: 20, height: 20 }} onChange={(e) => setDraft({ ...draft, autoCalc: e.target.checked })} />
            <span>Авто-расчёт от цены продажи (базовой цены)</span>
          </label>
          {draft.autoCalc && <label className="field"><span>Наценка, % (отрицательная — скидка)</span><input type="number" step="any" value={draft.markup} onChange={(e) => setDraft({ ...draft, markup: e.target.value })} /></label>}
          <div className="h2" style={{ fontSize: 16 }}>Правила авто-выбора</div>
          <label className="field"><span>По количеству товаров в корзине, от</span><input type="number" min="0" step="any" value={draft.minQty} onChange={(e) => setDraft({ ...draft, minQty: e.target.value })} /></label>
          <label className="field"><span>По сумме корзины, от</span><input type="number" min="0" step="any" value={draft.minTotal} onChange={(e) => setDraft({ ...draft, minTotal: e.target.value })} /></label>
          <label className="field"><span>По методу оплаты</span>
            <select value={draft.paymentMethodId} onChange={(e) => setDraft({ ...draft, paymentMethodId: e.target.value })}>
              <option value="">Любой</option>
              {(methods.data ?? []).map((m) => <option key={m.id} value={m.id}>{m.name}</option>)}
            </select>
          </label>
          <label className="row" style={{ gap: 8, marginBottom: 12 }}>
            <input type="checkbox" checked={draft.byClient} style={{ width: 20, height: 20 }} onChange={(e) => setDraft({ ...draft, byClient: e.target.checked })} />
            <span>По клиенту (предпочитаемый тип цены клиента)</span>
          </label>
          {error && <div className="error" role="alert">{error}</div>}
          <div className="row">
            {draft.id && <button className="btn light" onClick={() => remove(draft.id as string)}>Удалить</button>}
            <button className="btn" disabled={!draft.name.trim()} onClick={save}>Сохранить</button>
          </div>
        </Modal>
      )}
      {prices && <PriceEditor type={prices} onClose={() => setPrices(null)} say={say} />}
      {toast}
    </>
  );
}

/** Ручные цены товаров по категории: пустое поле — цена считается автоматически (или берётся базовая). */
function PriceEditor({ type, onClose, say }: { type: PriceType; onClose: () => void; say: (t: string) => void }) {
  const s = useSession();
  const [store, setStore] = useState(s.storeId ?? '');
  const products = useLoad(() => api<Product[]>('/products'), []);
  const saved = useLoad(() => api<{ productId: string; storeId: string; price: number }[]>(`/price-types/${type.id}/prices`), [type.id]);
  const [vals, setVals] = useState<Record<string, string>>({});
  const [search, setSearch] = useState('');
  const NIL = '00000000-0000-0000-0000-000000000000';
  const scopeStore = type.scope === 'common' ? NIL : store;

  useEffect(() => {
    const m: Record<string, string> = {};
    for (const r of saved.data ?? []) if (r.storeId === scopeStore) m[r.productId] = String(r.price);
    setVals(m);
  }, [saved.data, scopeStore]);

  async function save() {
    const rows = (products.data ?? []).map((p) => {
      const was = (saved.data ?? []).find((r) => r.productId === p.id && r.storeId === scopeStore);
      const now = vals[p.id]?.trim() ? n(vals[p.id]) : null;
      return { p, was: was?.price ?? null, now };
    }).filter((r) => r.was !== r.now);
    try {
      await api(`/price-types/${type.id}/prices`, { method: 'PUT', body: rows.map((r) => ({ productId: r.p.id, storeId: type.scope === 'common' ? undefined : store, price: r.now })) });
      say('Успешно');
      saved.reload();
    } catch (e) {
      say(e instanceof Error ? e.message : 'Ошибка');
    }
  }

  const list = (products.data ?? []).filter((p) => !p.isService && p.name.toLowerCase().includes(search.toLowerCase())).slice(0, 100);
  return (
    <Modal onClose={onClose}>
      <div className="h2" style={{ marginTop: 0 }}>Цены: {type.name}</div>
      {type.scope === 'store' && <StoreChips stores={s.stores} value={store} onChange={setStore} />}
      <input type="search" placeholder="Поиск" value={search} onChange={(e) => setSearch(e.target.value)} style={{ marginBottom: 12 }} />
      {products.loading && <Spinner />}
      {list.map((p) => {
        const base = p.stores.find((x) => x.storeId === store)?.salePrice ?? p.stores[0]?.salePrice ?? 0;
        return (
          <div key={p.id} className="card row">
            <div className="grow"><div className="title">{p.name}</div><div className="sub">базовая {num(base)}</div></div>
            <input type="number" min="0" step="any" aria-label={`Цена ${p.name}`} placeholder="авто" value={vals[p.id] ?? ''} style={{ maxWidth: 110 }} onChange={(e) => setVals({ ...vals, [p.id]: e.target.value })} />
          </div>
        );
      })}
      <button className="btn" onClick={save}>Сохранить цены</button>
    </Modal>
  );
}
