import { useState } from 'react';
import { Link } from 'react-router-dom';
import { api, type ListItem, type Sale } from '../api';
import { rub } from '../money';
import { guessPainting, PaintingForm, type PaintingDraft } from '../PaintingForm';
import { SaleEntry, saleDay, saleTime } from '../SaleEntry';
import { useSession } from '../session';
import { Header, Modal, Spinner, useLoad, useToast } from '../ui';
import { emptyPainting, staffLabel } from './Paintings';

export default function SaleRegister() {
  const session = useSession();
  const [day, setDay] = useState(() => saleDay(new Date()));
  const [selected, setSelected] = useState('');
  const [entry, setEntry] = useState<Sale | 'new' | null>(null);
  const [painting, setPainting] = useState<PaintingDraft | null>(null);
  const [toast, say] = useToast();
  const storeId = selected || session.storeId || '';
  const canSell = session.can('sale.new');
  const from = `${day}T00:00:00+03:00`;
  const to = new Date(Date.parse(from) + 86_400_000).toISOString();
  const sales = useLoad(() => storeId && session.can('sale.history')
    ? api<Sale[]>('/sales', { query: { storeId, from, to } })
    : Promise.resolve([]), [storeId, day]);
  const methods = useLoad(() => api<ListItem[]>('/lists/payment_methods'), []);
  const labels = new Map((methods.data ?? []).map((m) => [m.id, m.name]));
  const rows = sales.loading || sales.error ? [] : (sales.data ?? []).filter((sale) => sale.storeId === storeId && saleDay(new Date(sale.saleDate)) === day);
  const total = rows.reduce((sum, sale) => sum + sale.total - sale.returnedAmount, 0);
  const paymentTotals = new Map<string, number>();
  function addPayment(id: string | null, amount: number) {
    const label = (id && labels.get(id)) || 'Без способа оплаты';
    paymentTotals.set(label, (paymentTotals.get(label) ?? 0) + amount);
  }
  for (const sale of rows) {
    addPayment(sale.paymentMethodId, sale.firstPaymentValue ?? sale.total);
    if (sale.secondPaymentMethodId) addPayment(sale.secondPaymentMethodId, sale.secondPaymentValue ?? 0);
  }

  if (!session.can('sale.history')) return <><Header title="Таблица продаж" /><div className="empty">Нет доступа к истории продаж</div></>;

  return <>
    <Header title="Таблица продаж" />
    <main className="page wide">
      <div className="row sale-register-controls">
        <label className="field"><span>День</span><input type="date" value={day} onChange={(e) => { if (e.target.value) setDay(e.target.value); }} /></label>
        <label className="field"><span>Филиал</span><select value={storeId} onChange={(e) => setSelected(e.target.value)}>
          {session.stores.map((store) => <option key={store.id} value={store.id}>{store.name}</option>)}
        </select></label>
        {canSell && <button type="button" className="btn inline" disabled={!storeId} onClick={() => setEntry('new')}>Добавить продажу</button>}
        <button type="button" className="btn light inline" onClick={sales.reload}>Обновить</button>
      </div>
      {sales.loading && <Spinner />}
      {sales.error && <p className="error">{sales.error}</p>}
      {!sales.loading && !sales.error && <div className="sale-register-scroll" tabIndex={0} aria-label="Продажи за выбранный день">
        <table className="table sale-register-table">
          <thead><tr><th scope="col">Время</th><th scope="col">Заказ</th><th scope="col">Кадры</th><th scope="col">Комментарий</th><th scope="col" className="num">Сумма</th><th scope="col">Оплата</th><th scope="col">Тип заказа</th><th scope="col">Сотрудник</th><th scope="col">Действия</th></tr></thead>
          <tbody>
            {rows.map((sale) => <tr key={sale.id}>
              <td>{saleTime(new Date(sale.saleDate))}</td>
              <td><Link to={`/sale/${sale.id}`}>{sale.items.map((i) => i.qty === 1 ? i.name : `${i.name} × ${i.qty}`).join(', ')}</Link></td>
              <td>{sale.frameNumbers || '—'}</td>
              <td>{sale.comment || '—'}</td>
              <td className="num sale-register-money">{rub(sale.total)}{sale.returnedAmount > 0 && <div className="sub">Возврат {rub(sale.returnedAmount)}</div>}</td>
              <td>{[sale.paymentMethodId, sale.secondPaymentMethodId].map((id) => id && labels.get(id)).filter(Boolean).join(' + ') || '—'}</td>
              <td>{sale.orderKind || '—'}</td>
              <td>{sale.userName}</td>
              <td>
                <div className="sale-register-actions">
                  {canSell && <button type="button" className="sale-register-action" onClick={() => setEntry(sale)}>Правка</button>}
                  {sale.items.some((i) => i.qty > i.returnedQty) && <Link className="sale-register-action sale-return-link" to={`/sale/${sale.id}?action=return`}>Возврат</Link>}
                </div>
              </td>
            </tr>)}
            {!rows.length && <tr><td colSpan={9}>За выбранный день продаж пока нет.</td></tr>}
          </tbody>
        </table>
      </div>}
      <div className="row sale-register-totals" aria-label="Итоги дня">
        {[...paymentTotals].map(([name, amount]) => <span key={name}>{name} <b>{rub(amount)}</b></span>)}
        <span>Продаж <b>{rows.length}</b></span>
        <span className="sale-register-total">Итого с учётом возвратов <b>{rub(total)}</b></span>
      </div>
      {entry && <Modal onClose={() => setEntry(null)}>
        <SaleEntry storeId={entry === 'new' ? storeId : entry.storeId} day={entry === 'new' ? day : saleDay(new Date(entry.saleDate))}
          sale={entry === 'new' ? undefined : entry} onCancel={() => setEntry(null)} onSaved={(sale) => {
            if (entry === 'new' && sale.orderKind === 'Картина') {
              const names = sale.items.map((item) => item.name);
              const store = session.stores.find((store) => store.id === sale.storeId);
              setPainting({
                ...emptyPainting(staffLabel(sale.userName, store?.name ?? '')),
                ...guessPainting(names),
                date: saleDay(new Date(sale.saleDate)).split('-').reverse().join('.'),
                comment: [...names, sale.frameNumbers, sale.comment].filter(Boolean).join(', ').slice(0, 300),
                payment: rub(sale.total),
              });
            }
            setDay(saleDay(new Date(sale.saleDate)));
            setEntry(null);
            sales.reload();
            say(entry === 'new' ? 'Продажа сохранена' : 'Изменения сохранены');
          }} />
      </Modal>}
      {painting && <Modal onClose={() => setPainting(null)}>
        <PaintingForm initial={painting} onCancel={() => setPainting(null)} onSaved={() => {
          setPainting(null);
          say('Картина записана в таблицу');
        }} />
      </Modal>}
      {toast}
    </main>
  </>;
}
