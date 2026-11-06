import { useEffect, useState } from 'react';
import { useNavigate, useSearchParams } from 'react-router-dom';
import { api, type ListItem, type Product } from '../api';
import { rub, round2 } from '../money';
import { ProductPicker } from '../ProductPicker';
import { useSession } from '../session';
import { Header, StoreChips, useLoad, useToast } from '../ui';

/** Новое списание: товар, филиал, количество, причина и сумма (по умолчанию количество × себестоимость). */
export default function WriteoffForm() {
  const [params] = useSearchParams();
  const s = useSession();
  const nav = useNavigate();
  const [toast, say] = useToast();
  const reasons = useLoad(() => api<ListItem[]>('/lists/writeoff_reasons'), []);
  const [store, setStore] = useState(params.get('storeId') ?? s.storeId ?? '');
  const [product, setProduct] = useState<Product | null>(null);
  const [picker, setPicker] = useState(false);
  const [qty, setQty] = useState('1');
  const [reason, setReason] = useState('');
  const [amount, setAmount] = useState('');
  const [comment, setComment] = useState('');
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const pid = params.get('product');
    if (pid) api<Product>(`/products/${pid}`).then(setProduct).catch(() => undefined);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const auto = product ? round2((Number(qty.replace(',', '.')) || 0) * product.costPrice) : 0;

  async function save() {
    setError(null);
    if (!reason) return setError('Выберите причину списания');
    try {
      await api('/writeoffs', {
        body: {
          storeId: store,
          productId: product?.id,
          qty: Number(qty.replace(',', '.')),
          reasonId: reason,
          amount: amount.trim() === '' ? undefined : Number(amount.replace(',', '.')),
          comment: comment || undefined,
        },
      });
      say('Успешно');
      setTimeout(() => nav('/writeoffs', { replace: true }), 700);
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Ошибка');
    }
  }

  return (
    <>
      <Header title="Новое списание" />
      <main className="page">
        <StoreChips stores={s.stores} value={store} onChange={setStore} />
        <span className="label">Товар</span>
        <div className="card click row" onClick={() => setPicker(true)}>
          <div className="grow title">{product?.name ?? 'Выбрать товар'}</div>
          <span className="link">{product ? 'Изменить' : ''}</span>
        </div>
        <label className="field" style={{ marginTop: 16 }}><span>Количество</span>
          <input type="number" min="0" step="any" value={qty} onChange={(e) => setQty(e.target.value)} />
        </label>
        <label className="field"><span>Причина</span>
          <select value={reason} onChange={(e) => setReason(e.target.value)}>
            <option value="">Выберите причину</option>
            {(reasons.data ?? []).map((r) => <option key={r.id} value={r.id}>{r.name}</option>)}
          </select>
        </label>
        <label className="field"><span>Сумма (по умолчанию {rub(auto)})</span>
          <input type="number" min="0" step="any" value={amount} placeholder={String(auto)} onChange={(e) => setAmount(e.target.value)} />
        </label>
        <label className="field"><span>Комментарий</span><input type="text" value={comment} onChange={(e) => setComment(e.target.value)} /></label>
        {error && <div className="error" role="alert">{error}</div>}
        <button className="btn" disabled={!product || !(Number(qty) > 0)} onClick={save}>Сохранить</button>
      </main>
      {picker && <ProductPicker storeId={store} skipServices onClose={() => setPicker(false)} onPick={(p) => { setProduct(p); setPicker(false); }} />}
      {toast}
    </>
  );
}
