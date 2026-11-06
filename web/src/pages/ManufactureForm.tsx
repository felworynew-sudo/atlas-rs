import { useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { api, type Product } from '../api';
import { num, rub } from '../money';
import { ProductPicker } from '../ProductPicker';
import { useSession } from '../session';
import { Header, StoreChips, useLoad, useToast } from '../ui';

interface Preview { ingredients: { productId: string; name: string | null; qty: number }[]; costTotal: number }

/** Новое производство: готовый товар, количество, партия; ингредиенты и себестоимость считает сервер. */
export default function ManufactureForm() {
  const s = useSession();
  const nav = useNavigate();
  const [toast, say] = useToast();
  const [store, setStore] = useState(s.storeId ?? '');
  const [product, setProduct] = useState<Product | null>(null);
  const [qty, setQty] = useState('1');
  const [batch, setBatch] = useState('');
  const [picker, setPicker] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const q = Number(qty.replace(',', '.'));
  const preview = useLoad(
    () => (product && q > 0 ? api<Preview>('/manufactures/preview', { query: { productId: product.id, qty: q } }) : Promise.resolve(null)),
    [product?.id, q],
  );

  async function save() {
    if (!product) return;
    setError(null);
    try {
      await api('/manufactures', { body: { storeId: store, productId: product.id, qty: q, batch: batch || undefined } });
      say('Успешно');
      setTimeout(() => nav('/manufactures', { replace: true }), 600);
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Ошибка');
    }
  }

  return (
    <>
      <Header title="Новое производство" />
      <main className="page">
        <StoreChips stores={s.stores} value={store} onChange={setStore} />
        <span className="label">Готовый товар</span>
        <button className="btn light" style={{ marginBottom: 16 }} onClick={() => setPicker(true)}>{product ? product.name : 'Выбрать товар'}</button>
        <label className="field"><span>Количество</span><input type="number" min="0" step="any" value={qty} onChange={(e) => setQty(e.target.value)} /></label>
        <label className="field"><span>Партия</span><input type="text" value={batch} onChange={(e) => setBatch(e.target.value)} /></label>
        {preview.error && <div className="error">{preview.error}</div>}
        {preview.data && (
          <>
            <div className="h2">Ингредиенты</div>
            {preview.data.ingredients.map((i) => <div key={i.productId} className="total-row"><span>{i.name}</span><span>{num(i.qty)}</span></div>)}
            <div className="total-row"><b>Себестоимость партии</b><b>{rub(preview.data.costTotal)}</b></div>
          </>
        )}
        {error && <div className="error" role="alert">{error}</div>}
        <button className="btn" disabled={!product || !(q > 0)} onClick={save}>Произвести</button>
      </main>
      {picker && <ProductPicker storeId={store} skipServices onClose={() => setPicker(false)} onPick={(p) => { setProduct(p); setPicker(false); }} />}
      {toast}
    </>
  );
}
