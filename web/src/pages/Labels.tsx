import { useState } from 'react';
import { api } from '../api';
import { Icon } from '../Icon';
import { rub } from '../money';
import { ProductPicker } from '../ProductPicker';
import { useSession } from '../session';
import { Header, StoreChips, useLoad, useToast } from '../ui';
import { todayLocal } from '../dates';

interface Settings { labelHeight?: number; fontSize?: number; priceFontSize?: number; printDate?: boolean }
interface Rendered { settings: Settings; labels: { productId: string; name: string; price: number; barcode: string | null; copies: number; date: string | null }[] }
interface Line { productId: string; name: string; copies: string; price?: number }

/** Печать этикеток: выбор товаров и числа копий, настройки макета, печать из браузера. */
export default function Labels() {
  const s = useSession();
  const [toast, say] = useToast();
  const [store, setStore] = useState(s.storeId ?? '');
  const [lines, setLines] = useState<Line[]>([]);
  const [picker, setPicker] = useState(false);
  const [date, setDate] = useState(todayLocal());
  const settings = useLoad(() => api<Settings>('/label-settings'), []);
  const [draft, setDraft] = useState<Settings | null>(null);
  const cfg = draft ?? settings.data ?? {};

  async function saveSettings() {
    try {
      await api('/label-settings', { method: 'PUT', body: cfg });
      setDraft(null);
      settings.reload();
      say('Успешно');
    } catch (e) {
      say(e instanceof Error ? e.message : 'Ошибка');
    }
  }

  async function print() {
    try {
      const r = await api<Rendered>('/labels/render', { body: { storeId: store, date, items: lines.map((l) => ({ productId: l.productId, copies: Number(l.copies) })) } });
      const st = r.settings;
      const esc = (t: string) => t.replace(/[&<>]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;' })[c] as string);
      const html = r.labels
        .flatMap((l) => Array.from({ length: l.copies }, () => l))
        .map((l) => `<div class="l"><div class="n">${esc(l.name)}</div><div class="p">${esc(rub(l.price))}</div>${l.barcode ? `<div class="b">${esc(l.barcode)}</div>` : ''}${l.date ? `<div class="d">${esc(l.date)}</div>` : ''}</div>`)
        .join('');
      const w = window.open('', '_blank');
      if (!w) return say('Разрешите всплывающие окна для печати');
      w.document.write(`<!doctype html><meta charset="utf-8"><style>body{margin:0;font-family:sans-serif}.l{height:${st.labelHeight ?? 40}mm;padding:2mm;box-sizing:border-box;page-break-after:always;overflow:hidden}.n{font-size:${st.fontSize ?? 12}pt}.p{font-size:${st.priceFontSize ?? 18}pt;font-weight:bold}.b,.d{font-size:8pt}</style>${html}<script>onload=()=>print()</script>`);
      w.document.close();
    } catch (e) {
      say(e instanceof Error ? e.message : 'Ошибка');
    }
  }

  const num = (k: keyof Settings) => (
    <input type="number" min="0" step="any" value={String(cfg[k] ?? '')} onChange={(e) => setDraft({ ...cfg, [k]: Number(e.target.value) })} />
  );

  return (
    <>
      <Header title="Печать этикеток" />
      <main className="page">
        <StoreChips stores={s.stores} value={store} onChange={(id) => { setStore(id); setLines([]); }} />
        {lines.map((l, i) => (
          <div key={l.productId} className="card row">
            <div className="grow title">{l.name}</div>
            <input type="number" min="1" max="1000" aria-label="Копий" value={l.copies} style={{ maxWidth: 90 }} onChange={(e) => setLines((cur) => cur.map((x, k) => (k === i ? { ...x, copies: e.target.value } : x)))} />
            <button className="icon-btn small" aria-label="Убрать" onClick={() => setLines((cur) => cur.filter((_, k) => k !== i))}><Icon name="trash" size={18} /></button>
          </div>
        ))}
        <button className="btn light" style={{ margin: '8px 0 16px' }} onClick={() => setPicker(true)}>+ Добавить товар</button>
        <label className="field"><span>Дата на этикетке</span><input type="date" value={date} onChange={(e) => setDate(e.target.value)} /></label>
        <button className="btn" disabled={lines.length === 0 || lines.some((l) => !(Number(l.copies) >= 1))} onClick={print}>Печать</button>

        <div className="h2">Предпросмотр</div>
        {(() => {
          const l = lines[0];
          const mm = 3.78; // px на мм при 96 dpi
          return (
            <div style={{ background: '#fff', color: '#000', borderRadius: 6, width: 200 * 1, height: Math.min(Math.max((cfg.labelHeight ?? 40) * mm, 60), 300), padding: 8, overflow: 'hidden', boxSizing: 'border-box', marginBottom: 12 }}>
              <div style={{ fontSize: cfg.fontSize ?? 12 }}>{l?.name ?? 'Название товара'}</div>
              <div style={{ fontSize: cfg.priceFontSize ?? 18, fontWeight: 700 }}>{rub(l?.price ?? 0)}</div>
              {(cfg.printDate ?? true) && <div style={{ fontSize: 9 }}>{date}</div>}
            </div>
          );
        })()}
        <div className="h2">Настройки этикетки</div>
        <label className="field"><span>Высота, мм</span>{num('labelHeight')}</label>
        <label className="field"><span>Размер шрифта названия</span>{num('fontSize')}</label>
        <label className="field"><span>Размер шрифта цены</span>{num('priceFontSize')}</label>
        <label className="row" style={{ gap: 8, marginBottom: 12 }}>
          <input type="checkbox" checked={cfg.printDate ?? true} style={{ width: 20, height: 20 }} onChange={(e) => setDraft({ ...cfg, printDate: e.target.checked })} />
          <span>Печатать дату</span>
        </label>
        {draft && <button className="btn light" onClick={saveSettings}>Сохранить настройки</button>}
      </main>
      {picker && store && (
        <ProductPicker storeId={store} skipServices onClose={() => setPicker(false)} onPick={(p) => {
          setLines((cur) => (cur.some((l) => l.productId === p.id) ? cur : [...cur, { productId: p.id, name: p.name, copies: '1', price: p.stores.find((x) => x.storeId === store)?.salePrice ?? 0 }]));
          setPicker(false);
        }} />
      )}
      {toast}
    </>
  );
}
