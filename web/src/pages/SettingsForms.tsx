import { useEffect, useState } from 'react';
import { api, type ListItem, type Org, type Wallet } from '../api';
import { useSession } from '../session';
import { Header, Spinner, useLoad, useToast } from '../ui';
import { MODULE_TITLES } from './Roles';

type Kind = 'organization' | 'loyalty' | 'sales';

export default function SettingsForms({ kind }: { kind: Kind }) {
  if (kind === 'organization') return <Organization />;
  return <JsonSettings kind={kind} />;
}

/** «Настройки организации»: название и используемые модули (плитка модуля появляется на главной). */
function Organization() {
  const s = useSession();
  const [toast, say] = useToast();
  const [name, setName] = useState(s.org?.name ?? '');
  const [modules, setModules] = useState<Record<string, boolean>>(s.org?.modules ?? {});
  const [error, setError] = useState<string | null>(null);

  async function save() {
    setError(null);
    try {
      await api<Org>(`/organizations/${s.org?.id}`, { method: 'PUT', body: { name, modules } });
      await s.reload();
      say('Успешно');
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Ошибка');
    }
  }

  return (
    <>
      <Header title="Настройки организации" />
      <main className="page">
        <label className="field"><span>Название</span><input type="text" value={name} onChange={(e) => setName(e.target.value)} /></label>
        <div className="h2" style={{ fontSize: 18 }}>Используемые модули</div>
        {Object.entries(MODULE_TITLES).map(([key, title]) => (
          <label key={key} className="row" style={{ marginBottom: 12 }}>
            <input type="checkbox" checked={modules[key] === true} onChange={(e) => setModules({ ...modules, [key]: e.target.checked })} style={{ width: 22, height: 22 }} />
            <span>{title}</span>
          </label>
        ))}
        {error && <div className="error" role="alert">{error}</div>}
        <button className="btn" disabled={!name.trim()} onClick={save}>Сохранить</button>
      </main>
      {toast}
    </>
  );
}

const LOYALTY = [
  { key: 'cashbackEnabled', label: 'Кэшбэк (бонусы за покупки)', type: 'bool' },
  { key: 'defaultBonusLevel', label: 'Стартовый процент бонусов нового клиента, %', type: 'num' },
  { key: 'punchCardEnabled', label: 'Карта «N-й товар в подарок»', type: 'bool' },
  { key: 'punchCardCapacity', label: 'Сколько отметок до подарка (N)', type: 'num' },
] as const;

const SALES = [
  { key: 'deliveryPrice', label: 'Стоимость доставки', type: 'num' },
  { key: 'deliveryPaidByCustomer', label: 'Доставку оплачивает покупатель', type: 'bool' },
  { key: 'deliveryIncludeInReceipt', label: 'Отображать доставку в чеке', type: 'bool' },
  { key: 'deliveryPaymentSourceWallet', label: 'Источник оплаты доставки (счёт)', type: 'wallet' },
  { key: 'defaultDeliverySpendingCategory', label: 'Категория расходов для доставки', type: 'spendcat' },
  { key: 'showSaveCartForLater', label: 'Показывать «Отложить»', type: 'bool' },
  { key: 'mergeSimilarItems', label: 'Объединять одинаковые позиции', type: 'bool' },
  { key: 'allowMultipleProductSelection', label: 'Выбор нескольких товаров сразу', type: 'bool' },
  { key: 'dateFieldVisible', label: 'Поле «Дата» в продаже', type: 'bool' },
  { key: 'storeFieldVisible', label: 'Поле «Филиал» в продаже', type: 'bool' },
  { key: 'tableNumberFieldVisible', label: 'Поле «Номер стола»', type: 'bool' },
  { key: 'commentFieldVisible', label: 'Поле «Комментарий»', type: 'bool' },
  { key: 'showTotalPurchasePriceInCart', label: 'Показывать общую закупочную цену в чеке', type: 'bool' },
  { key: 'showPurchasePriceInEditCartItem', label: 'Показывать закупочную цену при правке позиции', type: 'bool' },
] as const;

/** Программа лояльности и настройки продаж: плоский JSON организации, отправляется целиком (PATCH). */
function JsonSettings({ kind }: { kind: 'loyalty' | 'sales' }) {
  const [toast, say] = useToast();
  const path = kind === 'loyalty' ? '/loyalty-settings' : '/sale-settings';
  const fields = kind === 'loyalty' ? LOYALTY : SALES;
  const data = useLoad(() => api<Record<string, unknown>>(path), [path]);
  const wallets = useLoad(() => (kind === 'sales' ? api<Wallet[]>('/wallets') : Promise.resolve([] as Wallet[])), [kind]);
  const cats = useLoad(() => (kind === 'sales' ? api<ListItem[]>('/lists/spending_categories') : Promise.resolve([] as ListItem[])), [kind]);
  const [v, setV] = useState<Record<string, string | boolean>>({});
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!data.data) return;
    const next: Record<string, string | boolean> = {};
    for (const f of fields) {
      const raw = data.data[f.key];
      next[f.key] = f.type === 'bool' ? raw === true : raw === undefined || raw === null ? '' : String(raw);
    }
    setV(next);
  }, [data.data, fields]);

  async function save() {
    setError(null);
    const body: Record<string, unknown> = {};
    for (const f of fields) body[f.key] = f.type === 'bool' ? v[f.key] === true : f.type === 'wallet' || f.type === 'spendcat' ? (String(v[f.key] ?? '') || null) : Number(String(v[f.key]).replace(',', '.'));
    try {
      await api(path, { method: 'PATCH', body });
      say('Успешно');
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Ошибка');
    }
  }

  return (
    <>
      <Header title={kind === 'loyalty' ? 'Программа лояльности' : 'Настройки продаж'} />
      <main className="page">
        {data.loading && <Spinner />}
        {!data.loading && fields.map((f) =>
          f.type === 'bool' ? (
            <label key={f.key} className="row" style={{ marginBottom: 16 }}>
              <input type="checkbox" checked={v[f.key] === true} onChange={(e) => setV({ ...v, [f.key]: e.target.checked })} style={{ width: 22, height: 22 }} />
              <span>{f.label}</span>
            </label>
          ) : f.type === 'wallet' || f.type === 'spendcat' ? (
            <label key={f.key} className="field"><span>{f.label}</span>
              <select value={String(v[f.key] ?? '')} onChange={(e) => setV({ ...v, [f.key]: e.target.value })}>
                <option value="">Не выбрано</option>
                {(f.type === 'wallet' ? wallets.data ?? [] : cats.data ?? []).map((x) => <option key={x.id} value={x.id}>{x.name}</option>)}
              </select>
            </label>
          ) : (
            <label key={f.key} className="field"><span>{f.label}</span>
              <input type="number" min="0" step="any" value={String(v[f.key] ?? '')} onChange={(e) => setV({ ...v, [f.key]: e.target.value })} />
            </label>
          ),
        )}
        {error && <div className="error" role="alert">{error}</div>}
        {!data.loading && <button className="btn" onClick={save}>Сохранить</button>}
      </main>
      {toast}
    </>
  );
}
