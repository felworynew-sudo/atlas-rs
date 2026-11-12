import { useState } from 'react';
import { api, downloadCsv } from '../api';
import { rub } from '../money';
import { useSession } from '../session';
import { Header, StoreChips, useToast } from '../ui';

type Kind = 'products' | 'sales' | 'manufactures';
const KINDS: { id: Kind; label: string; hint: string }[] = [
  { id: 'products', label: 'Товары', hint: 'Колонки: название, цена, закупка, штрихкод, категория, поставщик, остаток' },
  { id: 'sales', label: 'Продажи', hint: 'Исторические продажи; импорт можно целиком отменить' },
  { id: 'manufactures', label: 'Производства', hint: 'Выпуск составных товаров' },
];

interface Result {
  dryRun: boolean;
  created?: number;
  updated?: number;
  sales?: number;
  manufactures?: number;
  items?: number;
  total?: number;
  batchId?: string | null;
  errors: { row: number; message: string }[];
  [k: string]: unknown;
}

/** Экспорт товаров и клиентов в CSV; импорт товаров, продаж и производств из CSV с пробным прогоном. */
export default function ExportImport() {
  const s = useSession();
  const [toast, say] = useToast();
  const [store, setStore] = useState(s.storeId ?? '');
  const [kind, setKind] = useState<Kind>('products');
  const [text, setText] = useState('');
  const [fileName, setFileName] = useState('');
  const [res, setRes] = useState<Result | null>(null);
  const [busy, setBusy] = useState(false);

  async function run(dryRun: boolean) {
    setBusy(true);
    try {
      setRes(await api<Result>(`/import/${kind}`, { body: { storeId: store, content: text, dryRun } }));
      if (!dryRun) say('Импорт выполнен');
    } catch (e) {
      say(e instanceof Error ? e.message : 'Ошибка');
    } finally {
      setBusy(false);
    }
  }

  async function revert(id: string) {
    if (!confirm('Отменить весь импорт? Продажи будут удалены, остатки вернутся.')) return;
    try {
      await api(`/import/batches/${id}`, { method: 'DELETE' });
      setRes(null);
      say('Импорт отменён');
    } catch (e) {
      say(e instanceof Error ? e.message : 'Ошибка');
    }
  }

  const exp = (path: string, name: string) => downloadCsv(path, { storeId: store }, name, name.endsWith('.xlsx') ? 'xlsx' : 'csv').catch((e: unknown) => say(e instanceof Error ? e.message : 'Ошибка'));

  return (
    <>
      <Header title="Экспорт и импорт" />
      <main className="page">
        <StoreChips stores={s.stores} value={store} onChange={setStore} />
        <div className="h2" style={{ marginTop: 0 }}>Экспорт</div>
        <div className="row" style={{ marginBottom: 16 }}>
          <button className="btn light" onClick={() => exp('/export/products', 'products.csv')}>Товары (CSV)</button>
          <button className="btn light" onClick={() => exp('/export/clients', 'clients.csv')}>Клиенты (CSV)</button>
          <button className="btn light" onClick={() => exp('/export/products', 'products.xlsx')}>Товары (Excel)</button>
          <button className="btn light" onClick={() => exp('/export/clients', 'clients.xlsx')}>Клиенты (Excel)</button>
        </div>

        <div className="h2">Импорт</div>
        <div className="chips">
          {KINDS.map((k) => <button key={k.id} className={`chip${kind === k.id ? ' active' : ''}`} onClick={() => { setKind(k.id); setRes(null); }}>{k.label}</button>)}
        </div>
        <div className="sub" style={{ marginBottom: 12 }}>{KINDS.find((k) => k.id === kind)?.hint}</div>
        <label className="field"><span>Файл CSV{fileName ? `: ${fileName}` : ''}</span>
          <input type="file" accept=".csv,text/csv,text/plain" onChange={async (e) => {
            const f = e.target.files?.[0];
            if (!f) return;
            setFileName(f.name);
            setText(await f.text());
            setRes(null);
          }} />
        </label>
        <div className="row">
          <button className="btn light" disabled={!text || busy} onClick={() => run(true)}>Проверить</button>
          <button className="btn" disabled={!text || busy} onClick={() => run(false)}>Импортировать</button>
        </div>

        {res && (
          <div className="card" style={{ marginTop: 16 }}>
            <div className="title">{res.dryRun ? 'Пробный прогон (ничего не сохранено)' : 'Импорт выполнен'}</div>
            {kind === 'sales' ? (
              <div className="sub">Продаж: {res.sales ?? 0} · позиций: {res.items ?? 0} · на сумму {rub(res.total ?? 0)}</div>
            ) : (
              <div className="sub">{kind === 'manufactures' ? `Производств: ${res.manufactures ?? 0}` : `Создано: ${res.created ?? 0} · обновлено: ${res.updated ?? 0}`}</div>
            )}
            {res.errors.length > 0 && (
              <div className="error" role="alert" style={{ marginTop: 8 }}>
                {res.errors.slice(0, 20).map((e, i) => <div key={i}>{e.message}</div>)}
                {res.errors.length > 20 && <div>…и ещё {res.errors.length - 20}</div>}
              </div>
            )}
            {res.batchId && <button className="link" onClick={() => revert(res.batchId as string)}>Отменить этот импорт</button>}
          </div>
        )}
      </main>
      {toast}
    </>
  );
}
