import { useNavigate } from 'react-router-dom';
import { api, type HomeStats } from '../api';
import { Icon, type IconName } from '../Icon';
import { num, rub } from '../money';
import { useSession } from '../session';
import { useLoad } from '../ui';

interface Tile {
  title: string;
  icon: IconName;
  to?: string; // нет маршрута — экран ещё не перенесён
  perm: string;
  module?: string;
}

// Порядок плиток — как на главной оригинала.
const TILES: Tile[] = [
  { title: 'Новая продажа', icon: 'plus', to: '/new-sale', perm: 'sale.new', module: 'simple_sales' },
  { title: 'История продаж', icon: 'clock', to: '/sales-history', perm: 'sale.history', module: 'simple_sales' },
  { title: 'Комплексные продажи', icon: 'file-text', perm: 'complex_sale.view', module: 'complex_sales' },
  { title: 'Списания', icon: 'trash', perm: 'writeoff.history', module: 'writeoffs' },
  { title: 'Заказы от клиентов', icon: 'bell', perm: 'client_orders.view', module: 'marketplace' },
  { title: 'Остатки товаров', icon: 'clipboard', perm: 'stock.view', module: 'stocks' },
  { title: 'Товары, услуги и ингредиенты', icon: 'pricetags', to: '/products', perm: 'product.view' },
  { title: 'Расходы', icon: 'cart', perm: 'spending.view', module: 'spendings' },
  { title: 'Отчеты', icon: 'chart', perm: 'reports.view' },
  { title: 'Клиенты', icon: 'people', perm: 'client.view', module: 'clients' },
  { title: 'Ревизии', icon: 'edit', perm: 'revision.view', module: 'revisions' },
  { title: 'Настройки', icon: 'settings', perm: 'settings.edit' },
];

export default function Home() {
  const s = useSession();
  const nav = useNavigate();
  const statsOn = s.can('reports.home_stats') && !!s.storeId;
  const { data: stats } = useLoad(
    () => (statsOn ? api<HomeStats>('/reports/home', { query: { storeId: s.storeId } }) : Promise.resolve(null)),
    [s.storeId, statsOn],
  );
  const tiles = TILES.filter((t) => s.can(t.perm) && (!t.module || s.moduleOn(t.module)));

  return (
    <>
      <header className="header">
        <select aria-label="Филиал" value={s.storeId ?? ''} onChange={(e) => s.setStoreId(e.target.value)} style={{ width: 'auto', background: 'transparent', maxWidth: '60%' }}>
          {s.stores.map((st) => <option key={st.id} value={st.id}>{st.name}</option>)}
        </select>
        <h1 />
        <button className="link" onClick={() => confirm('Выйти из аккаунта?') && s.logout()}>{s.user?.name}</button>
      </header>
      <main className="page wide">
        <div className="org-name">{s.org?.name}</div>
        {stats && (
          <div className="stats">
            <div><b>Продажи на сегодня</b>{rub(stats.today.sales)}<br />{rub(stats.weekAgo.sales)}</div>
            <div><b>Количество чеков</b>{num(stats.today.checks)}<br />{num(stats.weekAgo.checks)}</div>
            <div><b>Средний чек</b>{rub(stats.today.avgCheck)}<br />{rub(stats.weekAgo.avgCheck)}</div>
          </div>
        )}
        <div className="tiles">
          {tiles.map((t) => (
            <button key={t.title} className={`tile${t.to ? '' : ' off'}`} onClick={() => t.to && nav(t.to)} title={t.to ? undefined : 'Экран ещё в разработке'} disabled={!t.to}>
              <Icon name={t.icon} size={50} />
              {t.title}
            </button>
          ))}
        </div>
      </main>
    </>
  );
}
