import { useState } from 'react';
import { siteApi, type Painting, type PaintingOptions } from './siteApi';

export type PaintingDraft = Omit<Painting, 'row' | 'tracker' | 'trackerLink'>;
const FALLBACK: PaintingOptions = {
  type: ['А3', 'АКРИЛ', 'ХОЛСТ', 'МЕТАЛЛ', 'КАРТМЕТ'],
  size: ['30х40 (А3)', '40х50', '50х60', '40х60', '50х70', '20х30', '15х20', '10х15', '40x40', 'светильник'],
  paymentStatus: ['полная оплата', 'предоплата', 'не оплачено'],
  status: ['Не согласован', 'Согласован', 'Передаю в печать', 'Передан в печать', 'Ждет клиента', 'Готов'],
};
const toIso = (d: string) => { const m = d.match(/^(\d{2})\.(\d{2})\.(\d{4})$/); return m ? `${m[3]}-${m[2]}-${m[1]}` : ''; };
const toRu = (d: string) => { const m = d.match(/^(\d{4})-(\d{2})-(\d{2})$/); return m ? `${m[3]}.${m[2]}.${m[1]}` : d; };
export const todayRu = () => toRu(new Date(Date.now() + 3 * 3600_000).toISOString().slice(0, 10));

/** Карточка картины: новая (row не задан) или правка строки листа «Печать». Сохраняет в таблицу через сайт. */
export function PaintingForm({ initial, row, options, onSaved, onCancel }: {
  initial: PaintingDraft; row?: number; options?: PaintingOptions | null; onSaved: (r: { row: number; tracker?: string }) => void; onCancel: () => void;
}) {
  const opt = options ?? FALLBACK;
  const [f, setF] = useState<PaintingDraft>(initial);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const set = (k: keyof PaintingDraft) => (e: { target: { value: string } }) => setF({ ...f, [k]: e.target.value });
  const withCurrent = (list: string[], v: string) => (v && !list.includes(v) ? [v, ...list] : list);

  async function save() {
    setBusy(true); setError(null);
    try {
      const r = row
        ? await siteApi<{ row: number }>('/api/dev/paintings', { method: 'PATCH', body: { row, fields: f } })
        : await siteApi<{ row: number; tracker: string }>('/api/dev/paintings', { body: f });
      onSaved(r);
    } catch (e) { setError(e instanceof Error ? e.message : 'Ошибка'); } finally { setBusy(false); }
  }

  return (
    <div>
      <h2 className="h2" style={{ marginTop: 0 }}>{row ? 'Картина' : 'Новая картина'}</h2>
      <div className="row wrap" style={{ alignItems: 'flex-start' }}>
        <label className="field grow" style={{ minWidth: 150 }}><span>Дата</span><input type="date" value={toIso(f.date)} onChange={(e) => setF({ ...f, date: toRu(e.target.value) })} /></label>
        <label className="field grow" style={{ minWidth: 150 }}><span>Тип</span>
          <select value={f.type} onChange={set('type')}><option value="">—</option>{withCurrent(opt.type, f.type).map((x) => <option key={x}>{x}</option>)}</select></label>
      </div>
      <label className="field"><span>Размер</span>
        <input type="text" list="painting-sizes" value={f.size} onChange={set('size')} placeholder="30х40 (А3)" />
        <datalist id="painting-sizes">{opt.size.map((x) => <option key={x} value={x} />)}</datalist></label>
      <div className="row wrap" style={{ alignItems: 'flex-start' }}>
        <label className="field grow" style={{ minWidth: 150 }}><span>Telegram клиента</span><input type="text" value={f.telegram} onChange={set('telegram')} placeholder="@ник" /></label>
        <label className="field grow" style={{ minWidth: 150 }}><span>Телефон</span><input type="text" value={f.phone} onChange={set('phone')} /></label>
      </div>
      <label className="field"><span>Комментарий</span><input type="text" value={f.comment} onChange={set('comment')} /></label>
      <div className="row wrap" style={{ alignItems: 'flex-start' }}>
        <label className="field grow" style={{ minWidth: 150 }}><span>Оплата</span><input type="text" value={f.payment} onChange={set('payment')} placeholder="5 750 ₽" /></label>
        <label className="field grow" style={{ minWidth: 150 }}><span>Статус оплаты</span>
          <select value={f.paymentStatus} onChange={set('paymentStatus')}><option value="">—</option>{withCurrent(opt.paymentStatus, f.paymentStatus).map((x) => <option key={x}>{x}</option>)}</select></label>
      </div>
      <label className="field"><span>Ссылка на Я.Диск</span><input type="text" value={f.disk} onChange={set('disk')} placeholder="https://disk.yandex.ru/…" /></label>
      <div className="row wrap" style={{ alignItems: 'flex-start' }}>
        <label className="field grow" style={{ minWidth: 150 }}><span>Сотрудник</span><input type="text" value={f.staff} onChange={set('staff')} /></label>
        <label className="field grow" style={{ minWidth: 150 }}><span>Готовность</span>
          <select value={f.status} onChange={set('status')}><option value="">—</option>{withCurrent(opt.status, f.status).map((x) => <option key={x}>{x}</option>)}</select></label>
      </div>
      {error && <div className="error" role="alert">{error}</div>}
      <div className="row">
        <button className="btn light" onClick={onCancel} disabled={busy}>Отмена</button>
        <button className="btn" onClick={() => void save()} disabled={busy || !f.date || !f.type}>{busy ? 'Сохраняю…' : 'Сохранить в таблицу'}</button>
      </div>
    </div>
  );
}

/** Догадка по названиям товаров в чеке: тип и размер картины. */
export function guessPainting(names: string[]): { type: string; size: string } {
  const t = names.join(' ').toLowerCase();
  const type = /акрил/.test(t) ? 'АКРИЛ' : /холст/.test(t) ? 'ХОЛСТ' : /металл/.test(t) ? 'МЕТАЛЛ' : /а3|a3|постер|печать/.test(t) ? 'А3' : '';
  const m = t.match(/(\d{2,3})\s*[хxна*×]+\s*(\d{2,3})/);
  const size = m ? (`${m[1]}х${m[2]}` === '30х40' ? '30х40 (А3)' : `${m[1]}х${m[2]}`) : '';
  return { type, size };
}
