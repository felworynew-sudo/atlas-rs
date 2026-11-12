import { api, type ProfitReport } from '../api';
import { monthStartLocal, todayLocal } from '../dates';
import { rub } from '../money';
import { CsvButton, Line, useReportFilters } from '../reportKit';
import { Header, Spinner, useLoad } from '../ui';

/** Прибыль / P&L: маржа, потери, расходы, ежемесячные расходы, операционная и чистая прибыль. */
export default function ProfitStats() {
  const f = useReportFilters(monthStartLocal(), todayLocal());
  const r = useLoad(() => api<ProfitReport>('/reports/profit', { query: f.query }), [f.store, f.from, f.to]);
  const p = r.data;
  const neg = (v: number) => (v > 0 ? `−${rub(v)}` : rub(v));

  return (
    <>
      <Header title="Прибыль / P&L" right={<CsvButton path="/reports/profit" query={f.query} file="profit-report.csv" />} />
      <main className="page">
        {f.ui}
        {r.loading && <Spinner />}
        {r.error && <div className="error">{r.error}</div>}
        {p && (
          <div className="card">
            <Line label="Продажи" value={rub(p.sales)} />
            <Line label="Себестоимость товаров" value={neg(p.costOfSales)} />
            <Line label="− Оплачено бонусами" value={neg(p.paidByBonus)} />
            <Line bold label="Маржа (Gross Income)" value={rub(p.grossIncome)} tone="good" />

            <div className="day">Возвраты, списания, недосдачи</div>
            <Line label="Возвраты" value={neg(p.returns)} tone="bad" />
            <Line label="Списания" value={neg(p.writeoffs)} tone="bad" />
            <Line label="Разница в ревизиях" value={rub(p.revisionDiff)} tone={p.revisionDiff < 0 ? 'bad' : undefined} />
            <Line label="Разница в закрытиях смен" value={rub(p.shiftDiff)} tone={p.shiftDiff < 0 ? 'bad' : undefined} />
            <Line label="Зарплаты" value={neg(p.salaries)} tone="bad" />
            <Line label="Комиссия банка" value={neg(p.bankCommission)} tone="bad" />

            <div className="day">Расходы</div>
            {p.expenses.byCategory.map((c) => <Line key={c.name} indent label={c.name} value={neg(c.amount)} />)}
            <Line bold label="Расходы" value={neg(p.expenses.total)} />
            <div className="day">Ежемесячные расходы</div>
            {p.monthlyExpenses.items.map((c) => <Line key={c.name} indent label={c.name} value={neg(c.amount)} />)}
            <Line bold label="Ежемесячные расходы" value={neg(p.monthlyExpenses.total)} />

            <div style={{ borderTop: '1px solid var(--fill-hover)', margin: '10px 0' }} />
            <div style={{ fontSize: 18 }}><Line bold label="Операционная прибыль (Net Income)" value={rub(p.operatingProfit)} tone={p.operatingProfit < 0 ? 'bad' : 'good'} /></div>
            <Line label="Капитальные вложения" value={neg(p.capitalExpenses)} />
            <div style={{ fontSize: 18 }}><Line bold label="Чистая прибыль" value={rub(p.netProfit)} tone={p.netProfit < 0 ? 'bad' : 'good'} /></div>

            <div className="day">Справочно</div>
            <Line label="Приемки" value={rub(p.supplies)} />
            <Line label="Перемещения" value={rub(p.transfers)} />
            <Line label="Стоимость товаров на начало" value={rub(p.stockValueStart)} />
            <Line label="Стоимость товаров на конец" value={rub(p.stockValueEnd)} />
            <Line label="Разница в стоимости" value={rub(p.stockValueDiff)} tone={p.stockValueDiff < 0 ? 'bad' : undefined} />
            {p.unknownStockItems > 0 && (
              <div className="sub" style={{ marginTop: 8 }}>Остатки неизвестны по {p.unknownStockItems} позициям: у них нет закупочной цены, поэтому стоимость товаров неполная.</div>
            )}
            <Line label="Чеков" value={String(p.checks)} />
          </div>
        )}
      </main>
    </>
  );
}
