import { useState } from 'react';
import { PaintingForm, todayRu, type PaintingDraft } from '../PaintingForm';
import { siteApi, type Painting, type PaintingOptions } from '../siteApi';
import { useSession } from '../session';
import { Header, Modal, Spinner, useLoad, useToast } from '../ui';

/** Сотрудник в формате таблицы: «Кирилл 0» (имя и этаж точки). */
export function staffLabel(name: string, storeName: string): string {
  const floor = storeName.match(/(\d+)\s*этаж/)?.[1];
  return floor ? `${name.split(' ')[0]} ${floor}` : name.split(' ')[0];
}

export const emptyPainting = (staff: string): PaintingDraft => ({
  date: todayRu(), type: '', size: '', telegram: '', phone: '', comment: '', payment: '', paymentStatus: 'полная оплата', disk: '', staff, status: 'Не согласован',
});

/** Картины — лист «Печать» Google-таблицы (по нему же работает трекер заказов): список, поиск, новая, правка. */
export default function Paintings() {
  const s = useSession();
  const store = s.stores.find((x) => x.id === s.storeId);
  const list = useLoad(() => siteApi<{ paintings: Painting[]; options: PaintingOptions }>('/api/dev/paintings?limit=300'), []);
  const [q, setQ] = useState('');
  const [filter, setFilter] = useState<'work' | 'ready' | 'all'>('work');
  const [edit, setEdit] = useState<Painting | 'new' | null>(null);
  const [toast, say] = useToast();

  const rows = (list.data?.paintings ?? []).filter((p) => {
    if (filter === 'work' && /готов/i.test(p.status)) return false;
    if (filter === 'ready' && !/готов/i.test(p.status)) return false;
    const t = q.trim().toLowerCase();
    return !t || [p.telegram, p.phone, p.comment, p.type, p.size, p.staff, p.tracker, p.payment].some((v) => v.toLowerCase().includes(t));
  });

  return (
    <>
      <Header title="Картины" right={<button className="chip small" onClick={() => setEdit('new')}>+ Новая</button>} />
      <main className="page wide">
        <input type="search" placeholder="Поиск: ник, телефон, номер трекера, сотрудник" value={q} onChange={(e) => setQ(e.target.value)} style={{ marginBottom: 12 }} />
        <div className="chips">
          {([['work', 'В работе'], ['ready', 'Готовые'], ['all', 'Все']] as const).map(([k, l]) => (
            <button key={k} className={`chip small${filter === k ? ' active' : ''}`} onClick={() => setFilter(k)}>{l}</button>
          ))}
        </div>
        {list.loading && <Spinner />}
        {list.error && <div className="error" role="alert">{list.error}</div>}
        {!list.loading && !list.error && rows.length === 0 && <div className="empty">Ничего не найдено</div>}
        {rows.map((p) => (
          <div key={p.row} className="card click" onClick={() => setEdit(p)}>
            <div className="row">
              <div className="grow">
                <div className="title">{p.type || '—'} · {p.size || 'размер ?'} {p.telegram && <span className="muted">· {p.telegram}</span>}</div>
                <div className="sub">{p.date} · {p.staff || 'сотрудник ?'}{p.payment ? ` · ${p.payment}` : ''}{p.tracker && !p.tracker.startsWith('#') ? ` · трекер ${p.tracker}` : ''}</div>
              </div>
              <span className={`chip small${/готов/i.test(p.status) ? '' : ' active'}`} style={{ pointerEvents: 'none' }}>{p.status || 'без статуса'}</span>
            </div>
          </div>
        ))}
      </main>
      {edit && (
        <Modal onClose={() => setEdit(null)}>
          <PaintingForm
            initial={edit === 'new' ? emptyPainting(staffLabel(s.user?.name ?? '', store?.name ?? '')) : edit}
            row={edit === 'new' ? undefined : edit.row}
            options={list.data?.options}
            onCancel={() => setEdit(null)}
            onSaved={(r) => { setEdit(null); list.reload(); say(r.tracker ? `Сохранено в таблицу, трекер ${r.tracker}` : 'Сохранено в таблицу'); }}
          />
        </Modal>
      )}
      {toast}
    </>
  );
}
