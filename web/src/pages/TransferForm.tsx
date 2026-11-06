import { useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { api } from '../api';
import { Icon } from '../Icon';
import { ProductPicker } from '../ProductPicker';
import { useSession } from '../session';
import { Header, useToast } from '../ui';

interface Line { productId: string; name: string; qty: string }

/** Новое перемещение: откуда, куда, товары с количеством. */
export default function TransferForm() {
  const s = useSession();
  const nav = useNavigate();
  const [toast, say] = useToast();
  const [fromStore, setFrom] = useState(s.storeId ?? '');
  const [toStore, setTo] = useState('');
  const [comment, setComment] = useState('');
  const [lines, setLines] = useState<Line[]>([]);
  const [picker, setPicker] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const valid = fromStore && toStore && fromStore !== toStore && lines.length > 0 && lines.every((l) => Number(l.qty) > 0);

  async function save() {
    setError(null);
    try {
      await api('/transfers', { body: { fromStoreId: fromStore, toStoreId: toStore, comment: comment || undefined, items: lines.map((l) => ({ productId: l.productId, qty: Number(l.qty) })) } });
      say('Успешно');
      setTimeout(() => nav('/transfers', { replace: true }), 600);
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Ошибка');
    }
  }

  return (
    <>
      <Header title="Новое перемещение" />
      <main className="page">
        <label className="field"><span>Откуда</span>
          <select value={fromStore} onChange={(e) => setFrom(e.target.value)}>{s.stores.map((st) => <option key={st.id} value={st.id}>{st.name}</option>)}</select>
        </label>
        <label className="field"><span>Куда</span>
          <select value={toStore} onChange={(e) => setTo(e.target.value)}>
            <option value="">Выберите филиал</option>
            {s.stores.filter((st) => st.id !== fromStore).map((st) => <option key={st.id} value={st.id}>{st.name}</option>)}
          </select>
        </label>
        {lines.map((l, i) => (
          <div key={l.productId} className="card row">
            <div className="grow title">{l.name}</div>
            <input type="number" min="0" step="any" aria-label="Количество" value={l.qty} style={{ maxWidth: 100 }} onChange={(e) => setLines((cur) => cur.map((x, k) => (k === i ? { ...x, qty: e.target.value } : x)))} />
            <button className="icon-btn small" aria-label="Убрать" onClick={() => setLines((cur) => cur.filter((_, k) => k !== i))}><Icon name="trash" size={18} /></button>
          </div>
        ))}
        <button className="btn light" style={{ margin: '8px 0 16px' }} onClick={() => setPicker(true)}>+ Добавить товар</button>
        <label className="field"><span>Комментарий</span><input type="text" value={comment} onChange={(e) => setComment(e.target.value)} /></label>
        {error && <div className="error" role="alert">{error}</div>}
        <button className="btn" disabled={!valid} onClick={save}>Переместить</button>
      </main>
      {picker && fromStore && (
        <ProductPicker storeId={fromStore} skipServices onClose={() => setPicker(false)} onPick={(p) => {
          setLines((cur) => (cur.some((l) => l.productId === p.id) ? cur : [...cur, { productId: p.id, name: p.name, qty: '1' }]));
          setPicker(false);
        }} />
      )}
      {toast}
    </>
  );
}
