import { useState } from 'react';
import { api } from '../api';
import { Icon } from '../Icon';
import { INVOICE_FIELDS, type PrintTemplate } from '../printInvoice';
import { useSession } from '../session';
import { Header, Modal, Spinner, useLoad, useToast } from '../ui';

type P = PrintTemplate['params'];
interface Draft { id?: string; name: string; p: P }
const blank = (): Draft => ({ name: '', p: { storeId: '', title: 'Накладная', subtitle: '', pageSize: 'A4', margin: 10, fields: {}, signLeft: { on: false, text: '' }, signRight: { on: false, text: '' }, footer: '' } });

/** Настройки печати: шаблоны накладных (заголовок, размер страницы, включаемые поля, подписи, текст внизу). */
export default function PrintTemplates() {
  const s = useSession();
  const [toast, say] = useToast();
  const list = useLoad(() => api<PrintTemplate[]>('/lists/print_templates'), []);
  const [draft, setDraft] = useState<Draft | null>(null);
  const [error, setError] = useState<string | null>(null);

  const setP = (patch: Partial<P>) => draft && setDraft({ ...draft, p: { ...draft.p, ...patch } });

  async function save() {
    if (!draft) return;
    setError(null);
    try {
      await api(draft.id ? `/lists/print_templates/${draft.id}` : '/lists/print_templates', { method: draft.id ? 'PUT' : 'POST', body: { name: draft.name, params: draft.p } });
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
      await api(`/lists/print_templates/${id}`, { method: 'DELETE' });
      setDraft(null);
      list.reload();
    } catch (e) {
      say(e instanceof Error ? e.message : 'Ошибка');
    }
  }

  return (
    <>
      <Header title="Настройки печати" right={<button className="icon-btn" aria-label="Создать шаблон" onClick={() => setDraft(blank())}><Icon name="plus" /></button>} />
      <main className="page">
        {list.loading && <Spinner />}
        {!list.loading && (list.data ?? []).length === 0 && <div className="empty">Нет созданных шаблонов. Создайте первый шаблон</div>}
        {(list.data ?? []).map((t) => (
          <div key={t.id} className="card click" onClick={() => setDraft({ id: t.id, name: t.name, p: { ...blank().p, ...t.params } })}>
            <div className="title">{t.name}</div>
            <div className="sub">{t.params.title || 'Накладная'} · {t.params.pageSize || 'A4'} · {t.params.storeId ? s.stores.find((x) => x.id === t.params.storeId)?.name ?? 'филиал' : 'общий шаблон'}</div>
          </div>
        ))}
      </main>
      {draft && (
        <Modal onClose={() => setDraft(null)}>
          <div className="h2" style={{ marginTop: 0 }}>{draft.id ? 'Шаблон' : 'Создать шаблон'}</div>
          <label className="field"><span>Название шаблона</span><input type="text" value={draft.name} onChange={(e) => setDraft({ ...draft, name: e.target.value })} /></label>
          <label className="field"><span>Филиал</span>
            <select value={draft.p.storeId ?? ''} onChange={(e) => setP({ storeId: e.target.value })}>
              <option value="">Общий шаблон</option>
              {s.stores.map((st) => <option key={st.id} value={st.id}>{st.name}</option>)}
            </select>
          </label>
          <label className="field"><span>Заголовок накладной</span><input type="text" value={draft.p.title ?? ''} onChange={(e) => setP({ title: e.target.value })} /></label>
          <label className="field"><span>Подзаголовок накладной</span><input type="text" value={draft.p.subtitle ?? ''} onChange={(e) => setP({ subtitle: e.target.value })} /></label>
          <label className="field"><span>Размер страницы</span>
            <select value={draft.p.pageSize ?? 'A4'} onChange={(e) => setP({ pageSize: e.target.value })}>
              {['A4', 'A5', 'A6', '80mm'].map((x) => <option key={x}>{x}</option>)}
            </select>
          </label>
          <label className="field"><span>Отступы страницы, px</span><input type="number" min="0" value={draft.p.margin ?? 10} onChange={(e) => setP({ margin: Number(e.target.value) })} /></label>
          <div className="h2" style={{ fontSize: 16 }}>Включать поля</div>
          {INVOICE_FIELDS.map(([k, l]) => (
            <label key={k} className="row" style={{ gap: 8, marginBottom: 8 }}>
              <input type="checkbox" style={{ width: 20, height: 20 }} checked={draft.p.fields?.[k] ?? k !== 'vat'} onChange={(e) => setP({ fields: { ...(draft.p.fields ?? {}), [k]: e.target.checked } })} />
              <span>{l}</span>
            </label>
          ))}
          <div className="h2" style={{ fontSize: 16 }}>Подписи</div>
          {(['signLeft', 'signRight'] as const).map((k) => (
            <div key={k} className="row" style={{ marginBottom: 8, gap: 8 }}>
              <input type="checkbox" style={{ width: 20, height: 20 }} aria-label={k === 'signLeft' ? 'Подпись слева' : 'Подпись справа'} checked={draft.p[k]?.on === true} onChange={(e) => setP({ [k]: { ...(draft.p[k] ?? {}), on: e.target.checked } })} />
              <input type="text" placeholder={k === 'signLeft' ? 'Подпись слева' : 'Подпись справа'} value={draft.p[k]?.text ?? ''} onChange={(e) => setP({ [k]: { ...(draft.p[k] ?? {}), text: e.target.value } })} />
            </div>
          ))}
          <label className="field"><span>Дополнительный текст внизу</span><textarea rows={3} value={draft.p.footer ?? ''} onChange={(e) => setP({ footer: e.target.value })} /></label>
          {error && <div className="error" role="alert">{error}</div>}
          <div className="row">
            {draft.id && <button className="btn light" onClick={() => remove(draft.id as string)}>Удалить</button>}
            <button className="btn" disabled={!draft.name.trim()} onClick={save}>Сохранить</button>
          </div>
        </Modal>
      )}
      {toast}
    </>
  );
}
