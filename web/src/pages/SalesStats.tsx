import { useState } from 'react';
import { api, type SalesReport } from '../api';
import { BarChart } from '../BarChart';
import { daysAgoLocal, todayLocal } from '../dates';
import { num, rub } from '../money';
import { CsvButton, Line, useReportFilters } from '../reportKit';
import { Header, Spinner, useLoad } from '../ui';

type By = 'payment' | 'purchase' | 'user';
const GROUPS: { id: By; label: string }[] = [
  { id: 'payment', label: 'По методу оплаты' },
  { id: 'purchase', label: 'По методу покупки' },
  { id: 'user', label: 'По сотруднику' },
];

/** Отчёт по продажам: итоги за период с разбивкой по выбранной группировке и графики по дням. */
export default function SalesStats() {
  const f = useReportFilters(daysAgoLocal(6), todayLocal());
  const [by, setBy] = useState<By>('payment');
  const q = { ...f.query, groupBy: by };
  const r = useLoad(() => api<SalesReport>('/reports/sales', { query: q }), [f.store, f.from, f.to, by]);
  const d = r.data;
  const weekday = (iso: string) => new Date(`${iso}T12:00:00`).toLocaleDateString('ru-RU', { day: 'numeric', month: 'numeric', weekday: 'short' });

  return (
    <>
      <Header title="Отчет по продажам" right={<CsvButton path="/reports/sales" query={q} file="sales-report.csv" />} />
      <main className="page">
        {f.ui}
        <span className="label">Группировать по</span>
        <div className="chips">
          {GROUPS.map((g) => <button key={g.id} className={`chip small${by === g.id ? ' active' : ''}`} onClick={() => setBy(g.id)}>{g.label}</button>)}
        </div>
        {r.loading && <Spinner />}
        {r.error && <div className="error">{r.error}</div>}
        {d && (
          <>
            <div className="h2">За весь период</div>
            <div className="card">
              <Line bold label="Продажи" value={rub(d.totals.sales)} />
              {d.groups.filter((g) => g.sales > 0).map((g) => <Line key={g.name} indent label={`− ${g.name}`} value={rub(g.sales)} />)}
              <Line bold label="Количество продаж" value={num(d.totals.count)} />
              {d.groups.filter((g) => g.count > 0).map((g) => <Line key={g.name} indent label={`− ${g.name}`} value={num(g.count)} />)}
              <Line bold label="Средний чек" value={rub(d.totals.avgCheck)} />
              <Line bold label="Возвраты" value={rub(d.totals.returns)} tone={d.totals.returns > 0 ? 'bad' : undefined} />
              {d.groups.filter((g) => g.returns > 0).map((g) => <Line key={g.name} indent label={`− ${g.name}`} value={rub(g.returns)} />)}
              <Line bold label="Количество возвратов" value={num(d.totals.returnsCount)} />
            </div>
            {d.days.length === 0 && <div className="empty">Продаж за период нет</div>}
            {d.days.length > 0 && (
              <>
                <BarChart title="Продажи сумма" data={d.days.map((x) => ({ label: weekday(x.date), value: x.sales }))} format={(v) => (v >= 1000 ? `${Math.round(v / 100) / 10}k ₽` : `${Math.round(v)} ₽`)} />
                <BarChart title="Количество продаж" data={d.days.map((x) => ({ label: weekday(x.date), value: x.count }))} format={(v) => String(v)} />
              </>
            )}
          </>
        )}
      </main>
    </>
  );
}
