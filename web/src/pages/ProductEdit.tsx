import { useEffect, useState, type FormEvent } from 'react';
import { useNavigate, useParams } from 'react-router-dom';
import { api, type Category, type ListItem, type Product } from '../api';
import { Icon } from '../Icon';
import { ProductPicker } from '../ProductPicker';
import { useSession } from '../session';
import { Header, Spinner, useLoad, useToast } from '../ui';

interface Ref { id: string; name: string }
interface Form {
  name: string;
  isService: boolean;
  isComposite: boolean;
  productionType: 'ahead' | 'on_sale';
  ingredients: { productId: string; name: string; qty: string }[];
  hiddenInNewSale: boolean;
  purchasePrice: string;
  extraCost: string;
  categoryId: string;
  supplierId: string;
  unitId: string;
  barcodes: string;
  allowFractional: boolean;
  important: boolean;
  stockThreshold: string;
  prices: Record<string, { active: boolean; price: string; stock: string }>;
}

export default function ProductEdit() {
  const { id } = useParams();
  const isNew = !id || id === 'new';
  const s = useSession();
  const nav = useNavigate();
  const [toast, say] = useToast();
  const cats = useLoad(() => api<Category[]>('/product-categories'), []);
  const sups = useLoad(() => api<Ref[]>('/suppliers'), []);
  const units = useLoad(() => api<ListItem[]>('/product-units') as unknown as Promise<Ref[]>, []);
  const product = useLoad(() => (isNew ? Promise.resolve(null) : api<Product>(`/products/${id}`)), [id]);
  const [f, setF] = useState<Form | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [picker, setPicker] = useState(false);

  useEffect(() => {
    if (product.loading) return;
    const p = product.data;
    const prices: Form['prices'] = {};
    for (const st of s.stores) {
      const sp = p?.stores.find((x) => x.storeId === st.id);
      prices[st.id] = { active: sp?.active ?? true, price: sp ? String(sp.salePrice) : '', stock: '' };
    }
    setF({
      name: p?.name ?? '',
      isService: p?.isService ?? false,
      isComposite: p?.isComposite ?? false,
      productionType: p?.productionType ?? 'ahead',
      ingredients: (p?.ingredients ?? []).map((i) => ({ productId: i.productId, name: i.name, qty: String(i.qty) })),
      hiddenInNewSale: p?.hiddenInNewSale ?? false,
      purchasePrice: p ? String(p.purchasePrice) : '',
      extraCost: p ? String(p.extraCost) : '',
      categoryId: p?.categoryId ?? '',
      supplierId: p?.supplierId ?? '',
      unitId: p?.unitId ?? '',
      barcodes: p?.barcodes.join(', ') ?? '',
      allowFractional: p?.allowFractional ?? false,
      important: p?.important ?? false,
      stockThreshold: p ? String(p.stockThreshold) : '',
      prices,
    });
  }, [product.data, product.loading, s.stores]);

  if (!f) return <><Header title="Товар" /><Spinner /></>;
  const set = <K extends keyof Form>(k: K, v: Form[K]) => setF({ ...f, [k]: v });
  const n = (v: string) => (v.trim() === '' ? 0 : Number(v.replace(',', '.')));

  async function save(e: FormEvent) {
    e.preventDefault();
    if (!f) return;
    setError(null);
    try {
      const body = {
        name: f.name,
        isService: f.isService,
        isComposite: f.isComposite && !f.isService,
        productionType: f.isComposite && !f.isService ? f.productionType : undefined,
        ingredients: f.isComposite && !f.isService ? f.ingredients.map((i) => ({ productId: i.productId, qty: n(i.qty) })) : [],
        hiddenInNewSale: f.hiddenInNewSale,
        purchasePrice: n(f.purchasePrice),
        extraCost: n(f.extraCost),
        categoryId: f.categoryId || null,
        supplierId: f.supplierId || null,
        unitId: f.unitId || null,
        barcodes: f.barcodes.split(',').map((b) => b.trim()).filter(Boolean),
        allowFractional: f.allowFractional,
        important: f.important,
        stockThreshold: n(f.stockThreshold),
        stores: Object.entries(f.prices).map(([storeId, p]) => ({ storeId, active: p.active, salePrice: n(p.price) })),
      };
      const saved = isNew ? await api<Product>('/products', { body }) : await api<Product>(`/products/${id}`, { method: 'PUT', body });
      if (!f.isService) {
        for (const [storeId, p] of Object.entries(f.prices)) {
          if (p.stock.trim() !== '') await api(`/stocks/${saved.id}`, { method: 'PUT', body: { storeId, qty: n(p.stock) } });
        }
      }
      say('Успешно');
      if (isNew) nav(`/product/${saved.id}`, { replace: true });
      else product.reload();
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Ошибка');
    }
  }

  async function copy() {
    try {
      const c = await api<Product>(`/products/${id}/copy`, { method: 'POST' });
      say('Копия создана');
      nav(`/product/${c.id}`);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Ошибка');
    }
  }

  async function remove() {
    if (!confirm('Удалить товар?')) return;
    try {
      await api(`/products/${id}`, { method: 'DELETE' });
      nav('/products', { replace: true });
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Ошибка');
    }
  }

  return (
    <>
      <Header title={isNew ? 'Новый' : f.name || 'Товар'} right={!isNew ? <>{s.can('product.view') && <button className="link" onClick={copy}>Создать копию</button>}{s.can('product.delete') && <button className="icon-btn" aria-label="Удалить товар" onClick={remove}><Icon name="trash" /></button>}</> : undefined} />
      <form className="page" onSubmit={save}>
        <div className="h2" style={{ marginTop: 0 }}>Общие</div>
        <label className="field"><span>Название</span><input type="text" value={f.name} placeholder="Молоко 1л" onChange={(e) => set('name', e.target.value)} /></label>
        <div className="chips">
          <button type="button" className={`chip${!f.isService ? ' active' : ''}`} onClick={() => setF({ ...f, isService: false })}>Товар</button>
          <button type="button" className={`chip${f.isService ? ' active' : ''}`} onClick={() => setF({ ...f, isService: true, isComposite: false })}>Услуга</button>
        </div>
        {!f.isService && (
          <>
            <label className="row" style={{ marginBottom: 18 }}>
              <input type="checkbox" checked={f.isComposite} onChange={(e) => set('isComposite', e.target.checked)} style={{ width: 22, height: 22 }} />
              <span>Составной товар (из ингредиентов)</span>
            </label>
            {f.isComposite && (
              <>
                <div className="h2" style={{ marginTop: 0 }}>Состав</div>
                {f.ingredients.map((ing, i) => (
                  <div key={ing.productId} className="card row">
                    <div className="grow title">{ing.name}</div>
                    <input type="number" min="0" step="any" aria-label={`Количество ${ing.name}`} value={ing.qty} style={{ maxWidth: 100 }} onChange={(e) => set('ingredients', f.ingredients.map((x, k) => (k === i ? { ...x, qty: e.target.value } : x)))} />
                    <button type="button" className="icon-btn small" aria-label="Убрать ингредиент" onClick={() => set('ingredients', f.ingredients.filter((_, k) => k !== i))}><Icon name="trash" size={18} /></button>
                  </div>
                ))}
                <button type="button" className="btn light" style={{ marginBottom: 12 }} onClick={() => setPicker(true)}>+ Добавить ингредиент</button>
                <div className="chips">
                  <button type="button" className={`chip${f.productionType === 'ahead' ? ' active' : ''}`} onClick={() => set('productionType', 'ahead')}>Производится заранее</button>
                  <button type="button" className={`chip${f.productionType === 'on_sale' ? ' active' : ''}`} onClick={() => set('productionType', 'on_sale')}>Списывается при продаже</button>
                </div>
              </>
            )}
          </>
        )}
        <label className="row" style={{ marginBottom: 18 }}>
          <input type="checkbox" checked={f.hiddenInNewSale} onChange={(e) => set('hiddenInNewSale', e.target.checked)} style={{ width: 22, height: 22 }} />
          <span>Скрывать на экране Новой продажи (для ингредиентов)</span>
        </label>
        {f.isComposite ? (
          product.data && <div className="card"><div className="title">Себестоимость 1 шт — {product.data.costPrice} ₽</div><div className="sub">считается из состава</div></div>
        ) : (
        <label className="field"><span>Закупочная цена</span><input type="number" min="0" step="any" value={f.purchasePrice} placeholder="300" onChange={(e) => set('purchasePrice', e.target.value)} /></label>
        )}
        <label className="field"><span>Доп расходы к закупочной цене</span><input type="number" min="0" step="any" value={f.extraCost} onChange={(e) => set('extraCost', e.target.value)} /></label>
        {s.stores.map((st) => (
          <div key={st.id} className="field">
            <span className="label">Цена продажи {st.name}</span>
            <input type="number" min="0" step="any" aria-label={`Цена продажи ${st.name}`} value={f.prices[st.id]?.price ?? ''} onChange={(e) => set('prices', { ...f.prices, [st.id]: { ...f.prices[st.id], price: e.target.value } })} />
            <label className="row" style={{ marginTop: 8 }}>
              <input type="checkbox" checked={f.prices[st.id]?.active ?? true} onChange={(e) => set('prices', { ...f.prices, [st.id]: { ...f.prices[st.id], active: e.target.checked } })} style={{ width: 22, height: 22 }} />
              <span>Включен для {st.name}</span>
            </label>
          </div>
        ))}
        <label className="field"><span>Категория</span>
          <select value={f.categoryId} onChange={(e) => set('categoryId', e.target.value)}>
            <option value="">Без категории</option>
            {(cats.data ?? []).map((c) => <option key={c.id} value={c.id}>{c.name}</option>)}
          </select>
        </label>
        <label className="field"><span>Поставщик</span>
          <select value={f.supplierId} onChange={(e) => set('supplierId', e.target.value)}>
            <option value="">Не выбран</option>
            {(sups.data ?? []).map((c) => <option key={c.id} value={c.id}>{c.name}</option>)}
          </select>
        </label>
        <label className="field"><span>Единица измерения</span>
          <select value={f.unitId} onChange={(e) => set('unitId', e.target.value)}>
            <option value="">Штука</option>
            {(units.data ?? []).map((c) => <option key={c.id} value={c.id}>{c.name}</option>)}
          </select>
        </label>
        <label className="field"><span>Штрихкоды (через запятую)</span><input type="text" value={f.barcodes} onChange={(e) => set('barcodes', e.target.value)} /></label>

        {!f.isService && (
          <>
            <div className="h2">Для Остатков товаров</div>
            {s.stores.map((st) => (
              <label key={st.id} className="field"><span>Количество товара в наличии {st.name}</span>
                <input type="number" step="any" placeholder="оставить без изменений" value={f.prices[st.id]?.stock ?? ''} onChange={(e) => set('prices', { ...f.prices, [st.id]: { ...f.prices[st.id], stock: e.target.value } })} />
              </label>
            ))}
            <label className="row" style={{ marginBottom: 18 }}>
              <input type="checkbox" checked={f.allowFractional} onChange={(e) => set('allowFractional', e.target.checked)} style={{ width: 22, height: 22 }} />
              <span>Показывать остатки нецелыми числами</span>
            </label>
            <label className="row" style={{ marginBottom: 18 }}>
              <input type="checkbox" checked={f.important} onChange={(e) => set('important', e.target.checked)} style={{ width: 22, height: 22 }} />
              <span>Показывать в важных товарах</span>
            </label>
            <label className="field"><span>Количество при котором нужно подсветить красным</span><input type="number" min="0" step="any" value={f.stockThreshold} placeholder="1" onChange={(e) => set('stockThreshold', e.target.value)} /></label>
          </>
        )}
        {error && <div className="error" role="alert">{error}</div>}
        <button className="btn" disabled={!f.name.trim()}>Сохранить</button>
      </form>
      {picker && s.storeId && (
        <ProductPicker storeId={s.storeId} skipServices onClose={() => setPicker(false)} onPick={(pp) => {
          if (pp.id !== id && !f.ingredients.some((i) => i.productId === pp.id)) set('ingredients', [...f.ingredients, { productId: pp.id, name: pp.name, qty: '1' }]);
          setPicker(false);
        }} />
      )}
      {toast}
    </>
  );
}
