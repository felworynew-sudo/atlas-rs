import { useState } from 'react';
import { useSearchParams } from 'react-router-dom';
import { api, type ProductAgg, type ProductsReport } from '../api';
import { daysAgoLocal, todayLocal } from '../dates';
import { num, rub } from '../money';
import { CsvButton, useReportFilters } from '../reportKit';
import { Header, Spinner, useLoad } from '../ui';

type Col = { id: keyof ProductAgg; label: string; fmt: (v: number) => string };
const COLS: Col[] = [
  { id: 'soldQty', label: 'Продажи кол-во', fmt: num },
  { id: 'soldSum', label: 'Продажи сумма', fmt: rub },
  { id: 'writeoffQty', label: 'Списания кол-во', fmt: num },
  { id: 'writeoffSum', label: 'Списания сумма', fmt: rub },
  { id: 'supplyQty', label: 'Приемки кол-во', fmt: num },
  { id: 'supplySum', label: 'Приемки сумма', fmt: rub },
  { id: 'margin', label: 'Маржинальность', fmt: (v) => `${num(v)}%` },
  { id: 'profit', label: 'Прибыль', fmt: rub },
  { id: 'checks', label: 'Чеки кол-во', fmt: num },
  { id: 'cost', label: 'Себестоимость сумма', fmt: rub },
];

function Table({ title, rows, cols }: { title: string; rows: ProductAgg[]; cols: Col[] }) {
  return (
    <>
      <div className="h2">{title}</div>
      {rows.length === 0 && <div className="empty">Нет данных за период</div>}
      {rows.length > 0 && (
        <div style={{ overflowX: 'auto' }}>
          <table className="table">
            <thead><tr><th>Название</th>{cols.map((c) => <th key={c.id} className="num">{c.label}</th>)}</tr></thead>
            <tbody>
              {rows.map((r) => (
                <tr key={`${r.id}-${r.name}`}><td>{r.name}</td>{cols.map((c) => <td key={c.id} className="num">{c.fmt(Number(r[c.id]))}</td>)}</tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </>
  );
}

/** Отчёт по товарам и списаниям: три таблицы (категории, поставщики, товары) с выбором колонок. */
export default function ProductStats() {
  const [params] = useSearchParams();
  const writeoffsOnly = params.get('cols') === 'writeoffs';
  const f = useReportFilters(daysAgoLocal(6), todayLocal());
  const [cols, setCols] = useState<Set<string>>(new Set(writeoffsOnly ? ['writeoffQty', 'writeoffSum'] : ['soldQty', 'soldSum', 'writeoffQty', 'writeoffSum']));
  const r = useLoad(() => api<ProductsReport>('/reports/products', { query: f.query }), [f.store, f.from, f.to]);
  const shown = COLS.filter((c) => cols.has(c.id));
  const toggle = (id: string) => setCols((cur) => { const n = new Set(cur); if (n.has(id)) n.delete(id); else n.add(id); return n; });

  return (
    <>
      <Header title={writeoffsOnly ? 'Отчет по списаниям' : 'Отчет по товарам'} right={<CsvButton path="/reports/products" query={f.query} file="products-report.csv" />} />
      <main className="page wide">
        {f.ui}
        <span className="label">Колонки</span>
        <div className="chips">
          {COLS.map((c) => (
            <label key={c.id} className="row" style={{ minWidth: 200 }}>
              <input type="checkbox" checked={cols.has(c.id)} onChange={() => toggle(c.id)} style={{ width: 22, height: 22 }} />
              <span style={{ fontSize: 13, fontWeight: 600 }}>{c.label}</span>
            </label>
          ))}
        </div>
        {r.loading && <Spinner />}
        {r.error && <div className="error">{r.error}</div>}
        {r.data && (
          <>
            <Table title="По категориям товаров" rows={r.data.byCategory} cols={shown} />
            <Table title="По поставщикам" rows={r.data.bySupplier} cols={shown} />
            <Table title="По товарам" rows={r.data.byProduct} cols={shown} />
          </>
        )}
      </main>
    </>
  );
}
