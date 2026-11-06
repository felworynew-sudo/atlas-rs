import { Link, Navigate, useParams } from 'react-router-dom';
import { CrudList, type CrudConfig, type Field } from '../CrudList';
import { rub } from '../money';
import { useSession } from '../session';
import { Header } from '../ui';
import Roles from './Roles';
import SettingsForms from './SettingsForms';
import Users from './Users';

interface Section { id: string; title: string; perm: string }

// Разделы «Настроек» (как в оригинале: универсальные списки и формы).
const SECTIONS: Section[] = [
  { id: 'users', title: 'Сотрудники', perm: 'settings.users' },
  { id: 'roles', title: 'Роли и доступы', perm: 'settings.users' },
  { id: 'stores', title: 'Филиалы', perm: 'settings.edit' },
  { id: 'suppliers', title: 'Поставщики', perm: 'settings.suppliers' },
  { id: 'categories', title: 'Категории товаров', perm: 'settings.categories' },
  { id: 'units', title: 'Единицы измерения', perm: 'settings.edit' },
  { id: 'wallets', title: 'Денежные счета', perm: 'settings.edit' },
  { id: 'payment-methods', title: 'Методы оплаты', perm: 'settings.edit' },
  { id: 'purchase-methods', title: 'Методы покупки', perm: 'settings.edit' },
  { id: 'discounts', title: 'Скидки', perm: 'settings.edit' },
  { id: 'return-reasons', title: 'Причины возвратов', perm: 'settings.edit' },
  { id: 'writeoff-reasons', title: 'Причины списаний', perm: 'settings.edit' },
  { id: 'spending-categories', title: 'Категории расходов', perm: 'settings.edit' },
  { id: 'monthly-expenses', title: 'Ежемесячные расходы', perm: 'settings.edit' },
  { id: 'organization', title: 'Настройки организации', perm: 'settings.edit' },
  { id: 'loyalty', title: 'Программа лояльности', perm: 'settings.edit' },
  { id: 'sales', title: 'Настройки продаж', perm: 'settings.edit' },
];

const opt = (value: string, label: string) => ({ value, label });
const nameField: Field = { key: 'name', label: 'Название', type: 'text', required: true };

/** Конфигурации универсальных списков. */
function configs(stores: { id: string; name: string }[], can: (p: string) => boolean): Record<string, CrudConfig> {
  const edit = can('settings.edit');
  const storeOpts = stores.map((s) => opt(s.id, s.name));
  return {
    stores: {
      title: 'Филиалы',
      path: '/stores',
      canEdit: edit,
      fields: [
        nameField,
        { key: 'address', label: 'Адрес', type: 'text' },
        { key: 'autoCloseTime', label: 'Автозакрытие смены (ЧЧ:ММ)', type: 'text', placeholder: '00:00', defaultValue: '00:00' },
        { key: 'timezone', label: 'Часовой пояс', type: 'text', defaultValue: 'Europe/Moscow' },
        { key: 'fiscalProvider', label: 'Фискализация (ОФД)', type: 'select', options: [opt('none', 'Нет'), opt('light_kassa', 'Light Kassa'), opt('wipon_kassa', 'Wipon Kassa')], defaultValue: 'none' },
        { key: 'showStatsOnHome', label: 'Показывать статистику на главной', type: 'checkbox', defaultValue: true },
      ],
      subtitle: (r) => `${r.address || 'без адреса'} · ${String(r.timezone)}`,
    },
    suppliers: { title: 'Поставщики', path: '/suppliers', canEdit: can('settings.suppliers'), fields: [nameField] },
    categories: {
      title: 'Категории товаров',
      path: '/product-categories',
      canEdit: can('settings.categories'),
      fields: [nameField, { key: 'sortOrder', label: 'Порядок', type: 'number' }],
      subtitle: (r) => `порядок ${String(r.sortOrder)}`,
    },
    units: {
      title: 'Единицы измерения',
      path: '/product-units',
      canEdit: edit,
      fields: [{ ...nameField, placeholder: 'Штука' }, { key: 'shortName', label: 'Короткое обозначение', type: 'text', placeholder: 'шт' }],
      subtitle: (r) => String(r.shortName ?? ''),
    },
    wallets: {
      title: 'Денежные счета',
      path: '/wallets',
      canEdit: edit,
      deletable: true,
      fields: [
        nameField,
        { key: 'storeId', label: 'Филиал', type: 'select', nullable: true, options: [opt('', 'Общий'), ...storeOpts] },
        { key: 'isCashRegister', label: 'Наличные с кассы (входят в расчёт смены)', type: 'checkbox' },
      ],
      subtitle: (r) => `баланс ${rub(Number(r.balance))}${r.isCashRegister ? ' · касса' : ''}`,
    },
    'payment-methods': {
      title: 'Методы оплаты',
      path: '/lists/payment_methods',
      canEdit: edit,
      fields: [
        nameField,
        { key: 'params.kind', label: 'Категория', type: 'select', options: [opt('cash', 'Наличные'), opt('qr', 'QR'), opt('terminal', 'Терминал'), opt('other', 'Другое')], defaultValue: 'cash' },
        { key: 'params.commission', label: 'Комиссия, %', type: 'number', defaultValue: '0' },
      ],
      subtitle: (r) => `комиссия ${String((r.params as { commission?: number }).commission ?? 0)}%`,
    },
    'purchase-methods': {
      title: 'Методы покупки',
      path: '/lists/purchase_methods',
      canEdit: edit,
      fields: [nameField, { key: 'params.markupPercent', label: 'Надбавка, %', type: 'number', defaultValue: '0' }, { key: 'params.delivery', label: 'Доставка', type: 'checkbox' }],
      subtitle: (r) => `надбавка ${String((r.params as { markupPercent?: number }).markupPercent ?? 0)}%`,
    },
    discounts: {
      title: 'Скидки',
      path: '/lists/discounts',
      canEdit: edit,
      fields: [
        nameField,
        { key: 'params.type', label: 'Тип', type: 'select', options: [opt('percent', 'Процент'), opt('amount', 'Сумма')], defaultValue: 'percent' },
        { key: 'params.value', label: 'Размер', type: 'number', required: true },
      ],
      subtitle: (r) => { const p = r.params as { type?: string; value?: number }; return p.type === 'amount' ? rub(Number(p.value)) : `${String(p.value)}%`; },
    },
    'return-reasons': { title: 'Причины возвратов', path: '/lists/return_reasons', canEdit: edit, fields: [nameField] },
    'writeoff-reasons': { title: 'Причины списаний', path: '/lists/writeoff_reasons', canEdit: edit, fields: [nameField] },
    'spending-categories': { title: 'Категории расходов', path: '/lists/spending_categories', canEdit: edit, fields: [nameField] },
    'monthly-expenses': {
      title: 'Ежемесячные расходы',
      path: '/recurring-expenses',
      canEdit: edit,
      fields: [
        nameField,
        { key: 'amount', label: 'Сумма в месяц', type: 'number', required: true },
        { key: 'storeId', label: 'Филиал', type: 'select', options: storeOpts, defaultValue: stores[0]?.id ?? '' },
        { key: 'startDate', label: 'Дата начала', type: 'date', required: true },
        { key: 'endDate', label: 'Дата окончания (необязательно)', type: 'date', nullable: true },
      ],
      subtitle: (r) => `${rub(Number(r.amount))} в месяц · с ${String(r.startDate)}${r.endDate ? ` по ${String(r.endDate)}` : ''}`,
    },
  };
}

export default function Settings() {
  const { section } = useParams();
  const s = useSession();
  if (!section) {
    const visible = SECTIONS.filter((x) => s.can(x.perm));
    return (
      <>
        <Header title="Настройки" />
        <main className="page">
          {visible.map((x) => (
            <Link key={x.id} to={`/settings/${x.id}`} style={{ display: 'block', color: 'inherit' }}>
              <div className="card click"><div className="title" style={{ fontSize: 15 }}>{x.title}</div></div>
            </Link>
          ))}
        </main>
      </>
    );
  }
  const def = SECTIONS.find((x) => x.id === section);
  if (!def || !s.can(def.perm)) return <Navigate to="/settings" replace />;
  if (section === 'users') return <Users />;
  if (section === 'roles') return <Roles />;
  if (section === 'organization' || section === 'loyalty' || section === 'sales') return <SettingsForms kind={section} />;
  const cfg = configs(s.stores, s.can)[section];
  return cfg ? <CrudList key={section} config={cfg} /> : <Navigate to="/settings" replace />;
}
