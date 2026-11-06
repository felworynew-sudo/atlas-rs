import { Link } from 'react-router-dom';
import { api, type ProfitReport, type SalesReport, type StockSummary } from '../api';
import { rub, num } from '../money';
import { useReportFilters, Line } from '../reportKit';
import { todayLocal } from '../dates';
import { useSession } from '../session';
import { Header, Spinner, useLoad } from '../ui';

const CARDS = [
  { to: '/reports/sales', title: 'Отчет по продажам', text: 'Продажи по дням и методам оплат' },
  { to: '/reports/profit', title: 'Прибыль / P&L', text: 'Отчёт о прибылях и убытках' },
  { to: '/reports/products', title: 'Отчет по товарам', text: 'Какие товары и категории продаются лучше всего' },
  { to: '/reports/products?cols=writeoffs', title: 'Отчет по списаниям', text: 'Какие товары и категории списываются больше всего' },
  { to: '/reports/revisions', title: 'Отчет по ревизиям', text: 'Консолидированный отчёт по ревизиям' },
];

/** Главная отчётов: ссылки на отчёты и сводная таблица «Сегодня». */
export default function Reports() {
  const s = useSession();
  const f = useReportFilters(todayLocal(), todayLocal());
  const profit = useLoad(() => api<ProfitReport>('/reports/profit', { query: f.query }), [f.store, f.from, f.to]);
  const byPurchase = useLoad(() => api<SalesReport>('/reports/sales', { query: { ...f.query, groupBy: 'purchase' } }), [f.store, f.from, f.to]);
  const stockStore = f.store === 'all' ? s.storeId : f.store;
  const stock = useLoad(() => (s.can('stock.view') && stockStore ? api<StockSummary>('/stocks/summary', { query: { storeId: stockStore } }) : Promise.resolve(null)), [stockStore]);
  const p = profit.data;
  const sales = byPurchase.data;

  return (
    <>
      <Header title="Отчеты" />
      <main className="page">
        <div className="h2" style={{ marginTop: 0 }}>Все отчеты</div>
        {CARDS.map((c) => (
          <Link key={c.title} to={c.to} style={{ display: 'block', color: 'inherit' }}>
            <div className="card click"><div className="title" style={{ fontSize: 15 }}>{c.title}</div><div className="sub" style={{ fontSize: 15 }}>{c.text}</div></div>
          </Link>
        ))}

        <div className="h2">Сегодня</div>
        {f.ui}
        {(profit.loading || byPurchase.loading) && <Spinner />}
        {p && sales && (
          <div className="card" role="table" aria-label="Сводка за день">
            <Line bold label="Продажи" value={`${num(sales.totals.count)} · ${rub(sales.totals.sales)}`} />
            {sales.groups.map((g) => <Line key={g.name} indent label={`− ${g.name}`} value={`${num(g.count)} · ${rub(g.sales)}`} />)}
            <Line bold label="Маржа от продаж" value={rub(p.grossIncome)} tone="good" />
            <Line label="Возвраты" value={`${num(sales.totals.returnsCount)} · ${rub(sales.totals.returns)}`} />
            <Line label="Приемки" value={rub(p.supplies)} />
            <Line label="Списания" value={rub(p.writeoffs)} />
            <Line label="ЗП за день" value={rub(p.salaries)} />
            <Line label="Расходы" value={rub(p.expenses.total)} />
            {stock.data && <Line label="Товары в наличии" value={`${num(stock.data.unitsInStock)} · ${rub(stock.data.valueAtPurchase)} / ${rub(stock.data.valueAtSale)}`} />}
            <Line label="Ежемесячные расходы" value={rub(p.monthlyExpenses.total)} />
            <Line bold label="~ Прибыль за день" value={rub(p.operatingProfit)} tone={p.operatingProfit < 0 ? 'bad' : 'good'} />
          </div>
        )}
      </main>
    </>
  );
}
