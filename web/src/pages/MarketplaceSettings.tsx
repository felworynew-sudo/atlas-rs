import { useEffect, useState } from 'react';
import { api, type Category } from '../api';
import { useSession } from '../session';
import { Header, Spinner, useLoad, useToast } from '../ui';

interface Cfg {
  marketplaceEnabled?: boolean;
  linkEnabled?: boolean;
  description?: string;
  storeForMarketplace?: string | null;
  disableNotInStockProducts?: boolean;
  enabledCategories?: Record<string, boolean>;
}

/** Онлайн-каталог: включение, ссылка, описание, филиал для заказов и категории в каталоге. */
export default function MarketplaceSettings() {
  const s = useSession();
  const [toast, say] = useToast();
  const data = useLoad(() => api<Cfg>('/marketplace-settings'), []);
  const cats = useLoad(() => api<Category[]>('/product-categories'), []);
  const [c, setC] = useState<Cfg>({});
  const [error, setError] = useState<string | null>(null);
  useEffect(() => { if (data.data) setC(data.data); }, [data.data]);

  async function save() {
    setError(null);
    try {
      await api('/marketplace-settings', { method: 'PATCH', body: { ...c, storeForMarketplace: c.storeForMarketplace || null } });
      say('Успешно');
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Ошибка');
    }
  }

  const check = (k: 'marketplaceEnabled' | 'linkEnabled' | 'disableNotInStockProducts', label: string) => (
    <label className="row" style={{ marginBottom: 14, gap: 8 }}>
      <input type="checkbox" checked={c[k] === true} onChange={(e) => setC({ ...c, [k]: e.target.checked })} style={{ width: 22, height: 22 }} />
      <span>{label}</span>
    </label>
  );

  return (
    <>
      <Header title="Онлайн-каталог" />
      <main className="page">
        {data.loading && <Spinner />}
        {!data.loading && (
          <>
            {check('marketplaceEnabled', 'Показывать организацию в онлайн-каталоге')}
            {check('linkEnabled', 'Доступ к каталогу по ссылке')}
            {check('disableNotInStockProducts', 'Скрывать товары, которых нет в наличии')}
            <label className="field"><span>Описание</span><textarea rows={3} value={c.description ?? ''} onChange={(e) => setC({ ...c, description: e.target.value })} /></label>
            <label className="field"><span>Филиал для заказов из каталога</span>
              <select value={c.storeForMarketplace ?? ''} onChange={(e) => setC({ ...c, storeForMarketplace: e.target.value })}>
                <option value="">Не выбран</option>
                {s.stores.map((st) => <option key={st.id} value={st.id}>{st.name}</option>)}
              </select>
            </label>
            <div className="h2" style={{ fontSize: 18 }}>Категории в каталоге</div>
            {(cats.data ?? []).map((k) => (
              <label key={k.id} className="row" style={{ marginBottom: 10, gap: 8 }}>
                <input type="checkbox" checked={c.enabledCategories?.[k.id] !== false} style={{ width: 20, height: 20 }}
                  onChange={(e) => setC({ ...c, enabledCategories: { ...(c.enabledCategories ?? {}), [k.id]: e.target.checked } })} />
                <span>{k.name}</span>
              </label>
            ))}
            {error && <div className="error" role="alert">{error}</div>}
            <button className="btn" onClick={save}>Сохранить</button>
          </>
        )}
      </main>
      {toast}
    </>
  );
}
