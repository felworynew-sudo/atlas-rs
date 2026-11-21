import { useEffect, useState } from 'react';
import { PaintingForm, todayRu, type PaintingDraft } from '../PaintingForm';
import { siteApi, type Painting, type PaintingOptions } from '../siteApi';
import { useSession } from '../session';
import { Header, Modal, Spinner, useLoad, useToast, copyText } from '../ui';

/** Сотрудник в формате таблицы: «Кирилл 0» (имя и этаж точки). */
export function staffLabel(name: string, storeName: string): string {
  const floor = storeName.match(/(\d+)\s*этаж/)?.[1];
  return floor ? `${name.split(' ')[0]} ${floor}` : name.split(' ')[0];
}

export const emptyPainting = (staff: string): PaintingDraft => ({
  date: todayRu(), type: '', size: '', telegram: '', phone: '', comment: '', payment: '', paymentStatus: 'полная оплата', disk: '', staff, status: 'Не согласован',
});

/** Миниатюра картины: превью публичной ссылки Я.Диска (через бота). */
function Thumb({ url }: { url: string }) {
  const [preview, setPreview] = useState('');
  const [failed, setFailed] = useState(false);
  useEffect(() => {
    setPreview('');
    setFailed(false);
    if (!url || !/^https?:\/\//.test(url)) return;
    let alive = true;
    fetch(`/api/dev/paintings/thumb?url=${encodeURIComponent(url)}`, { cache: 'no-store' })
      .then((r) => r.json())
      .then((d: { ok?: boolean; preview?: string }) => { if (alive) { if (d?.ok && d.preview) setPreview(d.preview); else setFailed(true); } })
      .catch(() => { if (alive) setFailed(true); });
    return () => { alive = false; };
  }, [url]);
  return (
    <div
      className="painting-thumb"
      style={{ width: 48, height: 48, borderRadius: 8, overflow: 'hidden', flex: '0 0 auto', background: 'rgba(255,255,255,0.06)', marginRight: 12, display: 'flex', alignItems: 'center', justifyContent: 'center' }}
      title={url ? 'Файл картины на Я.Диске' : 'Файла на Диске пока нет'}
    >
      {preview
        ? <img src={preview} alt="" style={{ width: '100%', height: '100%', objectFit: 'cover' }} />
        : <span style={{ fontSize: 18, opacity: 0.35 }}>{failed || url ? '🖼' : '＋'}</span>}
    </div>
  );
}

const UPLOAD_HINT = 'Перетащите картинку на нужную плашку — файл загрузится на Я.Диск, привяжется к картине и будет готов к печати (JPG/PNG до 40 МБ). Повторная загрузка заменяет прежний файл этой картины';
const MAX_UPLOAD_MB = 40;

/** Картины — лист «Печать» Google-таблицы (по нему же работает трекер заказов): список, поиск, новая, правка. */
export default function Paintings() {
  const s = useSession();
  const store = s.stores.find((x) => x.id === s.storeId);
  const list = useLoad(() => siteApi<{ paintings: Painting[]; options: PaintingOptions }>('/api/dev/paintings?limit=300'), []);
  const [q, setQ] = useState('');
  const [filter, setFilter] = useState<'work' | 'ready' | 'all'>('work');
  const [edit, setEdit] = useState<Painting | 'new' | null>(null);
  const [dragRow, setDragRow] = useState<number | null>(null);
  const [busyRow, setBusyRow] = useState<number | null>(null);
  const [stage, setStage] = useState('');
  const [toast, say] = useToast();

  const rows = (list.data?.paintings ?? []).filter((p) => {
    if (filter === 'work' && /готов/i.test(p.status)) return false;
    if (filter === 'ready' && !/готов/i.test(p.status)) return false;
    const t = q.trim().toLowerCase();
    return !t || [p.telegram, p.phone, p.comment, p.type, p.size, p.staff, p.tracker, p.payment].some((v) => (v || '').toLowerCase().includes(t));
  });

  async function uploadToPainting(p: Painting, file: File) {
    if (busyRow !== null) return;
    if (!/^image\//.test(file.type)) { say('Это не картинка — нужен JPG, PNG или другой файл изображения'); return; }
    const sizeMb = file.size / (1024 * 1024);
    if (sizeMb > MAX_UPLOAD_MB) { say(`Файл ${sizeMb.toFixed(1)} МБ — больше лимита загрузки ${MAX_UPLOAD_MB} МБ. Уменьшите картинку (для печати хватает 300 dpi по размеру холста).`); return; }
    setBusyRow(p.row);
    setStage('Читаю файл…');
    try {
      const data = await new Promise<string>((resolve, reject) => {
        const reader = new FileReader();
        reader.onload = () => resolve(String(reader.result));
        reader.onerror = () => reject(new Error('Не удалось прочитать файл'));
        reader.readAsDataURL(file);
      });
      setStage('Загружаю на Я.Диск…');
      const ext = (file.name.split('.').pop() || 'jpg').toLowerCase().replace(/[^a-z0-9]/g, '') || 'jpg';
      const up = await siteApi<{ url?: string; replaced?: number }>('/api/dev/paintings/upload', { body: { row: p.row, name: file.name.replace(/\.[^.]+$/, ''), ext, data } });
      if (!up.url) throw new Error('Диск не вернул ссылку на файл');
      setStage('Привязываю к картине…');
      await siteApi('/api/dev/paintings', { method: 'PATCH', body: { row: p.row, fields: { disk: up.url } } });
      say(up.replaced
        ? `Готово: файл на Я.Диске заменён (прежний макет удалён) и привязан к картине (${file.name}) — можно передавать в печать`
        : `Готово: файл на Я.Диске обновлён и привязан к картине (${file.name}) — можно передавать в печать`);
      list.reload();
    } catch (e) {
      say(`Не удалось загрузить: ${(e as Error).message}`);
    } finally {
      setBusyRow(null);
      setStage('');
    }
  }

  return (
    <>
      <style>{`@keyframes painting-upload-bar { from { transform: translateX(-100%); } to { transform: translateX(250%); } }`}</style>
      <Header title="Картины" right={<button className="chip small" onClick={() => setEdit('new')}>+ Новая</button>} />
      <main className="page wide">
        <input type="search" placeholder="Поиск: ник, телефон, номер трекера, сотрудник" value={q} onChange={(e) => setQ(e.target.value)} style={{ marginBottom: 12 }} />
        <div className="chips">
          {([['work', 'В работе'], ['ready', 'Готовые'], ['all', 'Все']] as const).map(([k, l]) => (
            <button key={k} className={`chip small${filter === k ? ' active' : ''}`} onClick={() => setFilter(k)}>{l}</button>
          ))}
        </div>
        <div className="sub" style={{ margin: '4px 0 12px', opacity: 0.7 }}>🖼 {UPLOAD_HINT}</div>
        {list.loading && <Spinner />}
        {list.error && <div className="error" role="alert">{list.error}</div>}
        {!list.loading && !list.error && rows.length === 0 && <div className="empty">Ничего не найдено</div>}
        {rows.map((p) => {
          const isBusy = busyRow === p.row;
          const isDrag = dragRow === p.row;
          return (
            <div
              key={p.row}
              className="card click"
              onClick={() => { if (!isBusy) setEdit(p); }}
              onDragOver={(e) => { if (e.dataTransfer.types.includes('Files')) { e.preventDefault(); e.dataTransfer.dropEffect = 'copy'; if (!isBusy && dragRow !== p.row) setDragRow(p.row); } }}
              onDragLeave={() => setDragRow((r) => (r === p.row ? null : r))}
              onDrop={(e) => { e.preventDefault(); setDragRow(null); const f = e.dataTransfer.files?.[0]; if (f && !isBusy) void uploadToPainting(p, f); }}
              style={isDrag ? { boxShadow: 'inset 0 0 0 2px #c9a24b', background: 'rgba(201,162,75,0.10)', transition: 'background .12s' } : { transition: 'background .12s' }}
              title={isDrag ? 'Отпустите — загрузим файл на Я.Диск' : (isBusy ? stage : UPLOAD_HINT)}
            >
              <div className="row">
                <Thumb url={p.disk} />
                <div className="grow">
                  <div className="title">{p.type || '—'} · {p.size || 'размер ?'} {p.telegram && <span className="muted">· {p.telegram}</span>}</div>
                  <div className="sub">{p.date} · {p.staff || 'сотрудник ?'}{p.payment ? ` · ${p.payment}` : ''}{p.tracker && !p.tracker.startsWith('#') ? ` · трекер ${p.tracker}` : ''}</div>
                  {isDrag && <div className="sub" style={{ color: '#c9a24b', fontWeight: 600 }}>⬇ Отпустите, чтобы загрузить на Я.Диск</div>}
                  {isBusy && (
                    <div className="sub" style={{ color: '#c9a24b' }}>
                      <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                        <span>{stage}</span>
                      </div>
                      <div style={{ position: 'relative', height: 3, borderRadius: 3, overflow: 'hidden', background: 'rgba(201,162,75,0.22)', marginTop: 6, maxWidth: 260 }}>
                        <span style={{ position: 'absolute', inset: 0, width: '40%', background: '#c9a24b', borderRadius: 3, animation: 'painting-upload-bar 1.1s linear infinite' }} />
                      </div>
                    </div>
                  )}
                </div>
                {p.tracker && !p.tracker.startsWith('#') && (
                  <button
                    className="chip small"
                    style={{ marginRight: 8 }}
                    title={`https://wow-my-eyes.ru/track/${p.tracker}`}
                    onClick={(e) => {
                      e.stopPropagation();
                      void copyText(`https://wow-my-eyes.ru/track/${p.tracker}`).then((ok) => say(ok ? 'Ссылка на трекер скопирована' : 'Не удалось скопировать'));
                    }}
                  >
                    Ссылка
                  </button>
                )}
                <span className={`chip small painting-status${/готов/i.test(p.status) ? '' : ' active'}`}>{p.status || 'без статуса'}</span>
              </div>
            </div>
          );
        })}
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
