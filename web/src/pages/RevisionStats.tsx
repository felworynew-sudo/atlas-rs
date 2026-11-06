import { api, type RevisionLine, type RevisionsReport } from '../api';
import { daysAgoLocal, todayLocal } from '../dates';
import { num, rub } from '../money';
import { CsvButton, Line, useReportFilters } from '../reportKit';
import { Header, Spinner, useLoad } from '../ui';

function List({ title, rows }: { title: string; rows: RevisionLine[] }) {
  return (
    <>
      <div className="h2">{title}</div>
      {rows.length === 0 && <div className="empty">Нет данных за период</div>}
      {rows.length > 0 && (
        <div style={{ overflowX: 'auto' }}>
          <table className="table">
            <thead><tr><th>Товар</th><th className="num">было</th><th className="num">стало</th><th className="num">разница (закупка)</th><th className="num">разница (продажа)</th></tr></thead>
            <tbody>
              {rows.map((r, i) => (
                <tr key={`${r.name}-${i}`}><td>{r.name}</td><td className="num">{num(r.was)}</td><td className="num">{num(r.became)}</td><td className="num">{rub(r.diffAtPurchase)}</td><td className="num">{rub(r.diffAtSale)}</td></tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </>
  );
}

/** Отчёт по ревизиям: «в минусе», «в плюсе», баланс и два списка расхождений. */
export default function RevisionStats() {
  const f = useReportFilters(daysAgoLocal(6), todayLocal());
  const r = useLoad(() => api<RevisionsReport>('/reports/revisions', { query: f.query }), [f.store, f.from, f.to]);
  const d = r.data;

  return (
    <>
      <Header title="Отчет по ревизиям" right={<CsvButton path="/reports/revisions" query={f.query} file="revisions-report.csv" />} />
      <main className="page wide">
        {f.ui}
        {r.loading && <Spinner />}
        {r.error && <div className="error">{r.error}</div>}
        {d && (
          <>
            <div className="card" style={{ marginTop: 16 }}>
              <Line bold label="В минусе" value={rub(d.minus)} tone={d.minus < 0 ? 'bad' : undefined} />
              <Line bold label="В плюсе" value={rub(d.plus)} tone={d.plus > 0 ? 'good' : undefined} />
              <Line bold label="Баланс" value={rub(d.balance)} tone={d.balance < 0 ? 'bad' : d.balance > 0 ? 'good' : undefined} />
            </div>
            <List title="Список в минусе" rows={d.minusList} />
            <List title="Список в плюсе" rows={d.plusList} />
          </>
        )}
      </main>
    </>
  );
}
