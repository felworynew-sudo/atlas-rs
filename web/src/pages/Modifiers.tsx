import { useState } from 'react';
import { api, type ListItem } from '../api';
import { Icon } from '../Icon';
import { rub } from '../money';
import { ProductPicker } from '../ProductPicker';
import { useSession } from '../session';
import { Header, Modal, Spinner, useLoad, useToast } from '../ui';

export interface ModItem { productId: string; name: string; price: number }
export interface ModParams { items?: ModItem[]; stores?: Record<string, boolean> }

interface Draft { id?: string; name: string; stores: Record<string, boolean>; items: { productId: string; name: string; price: string }[] }

/** Модификаторы товаров: группы добавок (сироп, молоко…) — товары с ценой добавки; доступность по филиалам. На кассе добавляются в чек. */
export default function Modifiers() {
  const s = useSession();
  const [toast, say] = useToast();
  const list = useLoad(() => api<ListItem[]>('/lists/product_modifiers'), []);
  const [draft, setDraft] = useState<Draft | null>(null);
  const [picker, setPicker] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const edit = (m?: ListItem) => {
    const p = (m?.params ?? {}) as ModParams;
    setDraft({
      id: m?.id,
      name: m?.name ?? '',
      stores: Object.fromEntries(s.stores.map((st) => [st.id, p.stores?.[st.id] !== false])),
      items: (p.items ?? []).map((i) => ({ productId: i.productId, name: i.name, price: String(i.price) })),
    });
  };

  async function save() {
    if (!draft) return;
    setError(null);
    const body = { name: draft.name, params: { stores: draft.stores, items: draft.items.map((i) => ({ productId: i.productId, name: i.name, price: Number(i.price.replace(',', '.')) })) } };
    try {
      await api(draft.id ? `/lists/product_modifiers/${draft.id}` : '/lists/product_modifiers', { method: draft.id ? 'PUT' : 'POST', body });
      setDraft(null);
      list.reload();
      say('Успешно');
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Ошибка');
    }
  }

  async function remove(id: string) {
    if (!confirm('Удалить модификатор?')) return;
    try {
      await api(`/lists/product_modifiers/${id}`, { method: 'DELETE' });
      setDraft(null);
      list.reload();
    } catch (e) {
      say(e instanceof Error ? e.message : 'Ошибка');
    }
  }

  return (
    <>
      <Header title="Модификаторы товаров" right={<button className="icon-btn" aria-label="Новый модификатор" onClick={() => edit()}><Icon name="plus" /></button>} />
      <main className="page">
        {list.loading && <Spinner />}
        {!list.loading && (list.data ?? []).length === 0 && <div className="empty">Модификаторов пока нет</div>}
        {(list.data ?? []).map((m) => (
          <div key={m.id} className="card click" onClick={() => edit(m)}>
            <div className="title">{m.name}</div>
            <div className="sub">{((m.params as ModParams).items ?? []).map((i) => `${i.name} +${rub(i.price)}`).join(', ') || 'без добавок'}</div>
          </div>
        ))}
      </main>
      {draft && (
        <Modal onClose={() => setDraft(null)}>
          <div className="h2" style={{ marginTop: 0 }}>{draft.id ? 'Модификатор' : 'Новый модификатор'}</div>
          <label className="field"><span>Название</span><input type="text" value={draft.name} placeholder="Сиропы" onChange={(e) => setDraft({ ...draft, name: e.target.value })} /></label>
          <div className="h2" style={{ fontSize: 16 }}>Настройки по филиалам</div>
          {s.stores.map((st) => (
            <label key={st.id} className="row" style={{ gap: 8, marginBottom: 8 }}>
              <input type="checkbox" checked={draft.stores[st.id] !== false} style={{ width: 20, height: 20 }} onChange={(e) => setDraft({ ...draft, stores: { ...draft.stores, [st.id]: e.target.checked } })} />
              <span>Доступен в {st.name}</span>
            </label>
          ))}
          <div className="h2" style={{ fontSize: 16 }}>Модификаторы товаров</div>
          {draft.items.map((i, k) => (
            <div key={i.productId} className="card row">
              <div className="grow title">{i.name}</div>
              <input type="number" min="0" step="any" aria-label={`Цена ${i.name}`} value={i.price} style={{ maxWidth: 90 }} onChange={(e) => setDraft({ ...draft, items: draft.items.map((x, j) => (j === k ? { ...x, price: e.target.value } : x)) })} />
              <button className="icon-btn small" aria-label="Убрать" onClick={() => setDraft({ ...draft, items: draft.items.filter((_, j) => j !== k) })}><Icon name="trash" size={18} /></button>
            </div>
          ))}
          <button className="btn light" style={{ marginBottom: 12 }} onClick={() => setPicker(true)}>+ Добавить товар</button>
          {error && <div className="error" role="alert">{error}</div>}
          <div className="row">
            {draft.id && <button className="btn light" onClick={() => remove(draft.id as string)}>Удалить</button>}
            <button className="btn" disabled={!draft.name.trim() || draft.items.some((i) => !(Number(i.price) >= 0) || i.price === '')} onClick={save}>Сохранить</button>
          </div>
        </Modal>
      )}
      {picker && draft && s.storeId && (
        <ProductPicker storeId={s.storeId} onClose={() => setPicker(false)} onPick={(p) => {
          if (!draft.items.some((i) => i.productId === p.id)) setDraft({ ...draft, items: [...draft.items, { productId: p.id, name: p.name, price: String(p.stores.find((x) => x.storeId === s.storeId)?.salePrice ?? 0) }] });
          setPicker(false);
        }} />
      )}
      {toast}
    </>
  );
}
