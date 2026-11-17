import { useRef, useState, type FormEvent } from 'react';
import { api, type Client, type ListItem, type Product, type Sale, type Shift } from './api';
import { ClientPicker } from './ClientPicker';
import { rub } from './money';
import { useSession } from './session';
import { useLoad } from './ui';

const ORDER_KINDS = ['Другое', 'Аксессуар', 'Картина', 'Электронный формат', 'Печать'];
const zone = 'Europe/Moscow';
export const saleDay = (date: Date) => date.toLocaleDateString('en-CA', { timeZone: zone });
export const saleTime = (date: Date) => date.toLocaleTimeString('ru-RU', { timeZone: zone, hour: '2-digit', minute: '2-digit' });

type Line = { key: string; productId: string | null; name: string; qty: string; price: string };

export function SaleEntry({ storeId, day, sale, closedShift, onSaved, onCancel }: {
  storeId: string;
  day: string;
  sale?: Sale;
  closedShift?: Pick<Shift, 'id' | 'staff'>;
  onSaved: (sale: Sale) => void;
  onCancel: () => void;
}) {
  const session = useSession();
  const products = useLoad(() => api<Product[]>('/products', { query: { storeId } }), [storeId]);
  const payments = useLoad(() => api<ListItem[]>('/lists/payment_methods'), []);
  const saleId = sale?.id;
  const staff = useLoad(() => saleId
    ? api<{ id: string; name: string }[]>(`/sales/${saleId}/staff`)
    : Promise.resolve([]), [saleId]);
  const initialTime = saleTime(sale ? new Date(sale.saleDate) : new Date());
  const [date, setDate] = useState(day);
  const [time, setTime] = useState(initialTime);
  const [frames, setFrames] = useState(sale?.frameNumbers ?? '');
  const [comment, setComment] = useState(sale?.comment ?? '');
  const [kind, setKind] = useState(sale?.orderKind || 'Другое');
  const [payment, setPayment] = useState(sale?.paymentMethodId ?? '');
  const shiftStaff = (closedShift?.staff ?? []).filter((person) => session.can('shift.edit_data') || person.userId === session.user?.id);
  const [sellerId, setSellerId] = useState(sale?.userId
    ?? shiftStaff.find((person) => person.userId === session.user?.id)?.userId ?? shiftStaff[0]?.userId ?? '');
  const [client, setClient] = useState<Client | null>(null);
  const [clientPending, setClientPending] = useState(false);
  const [lines, setLines] = useState<Line[]>(() => sale?.items.map((line) => ({
    key: line.id, productId: line.productId, name: line.name, qty: String(line.qty), price: String(line.price),
  })) ?? []);
  const [term, setTerm] = useState('');
  const [changed, setChanged] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const idempotencyKey = useRef(crypto.randomUUID());
  const canEditAmounts = !sale || sale.canEditAmounts;
  const methods = payments.data ?? [];
  const defaultMethod = methods.find((x) => /терм|карт/i.test(x.name))?.id ?? methods[0]?.id ?? '';
  const chosenPayment = payment || (sale ? '' : defaultMethod);
  const catalog = (products.data ?? []).filter((p) => !p.hiddenInNewSale && p.name.toLowerCase().includes(term.toLowerCase()) &&
    p.stores.some((price) => price.storeId === storeId && price.active));

  function changeLine(key: string, fields: Partial<Line>) {
    setChanged(true);
    setLines((current) => current.map((line) => line.key === key ? { ...line, ...fields } : line));
  }

  async function save(event: FormEvent) {
    event.preventDefault();
    if (busy || clientPending) return;
    const items = lines.map(({ productId, name, qty, price }) => ({ productId, name, qty: Number(qty), price: Number(price) }));
    const saleDate = sale && date === day && time === initialTime ? sale.saleDate : new Date(`${date}T${time}:00+03:00`).toISOString();
    const fields = { saleDate, frameNumbers: frames, comment, orderKind: kind, paymentMethodId: chosenPayment || null };
    setBusy(true);
    setError('');
    try {
      const updated = sale
        ? await api<Sale>(`/sales/${sale.id}`, { method: 'PATCH', body: {
            ...fields, revision: sale.revision, userId: sellerId !== sale.userId ? sellerId : undefined, items: changed ? items : undefined,
          } })
        : await api<Sale>('/sales', { body: {
            ...fields, storeId, clientId: client?.id, items, idempotencyKey: idempotencyKey.current,
            shiftId: closedShift?.id, userId: closedShift ? sellerId : undefined,
          } });
      onSaved(updated);
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Не удалось сохранить продажу');
    } finally {
      setBusy(false);
    }
  }

  return (
    <form className="sale-entry" onSubmit={save}>
      <h2>{sale ? 'Редактировать продажу' : closedShift ? 'Продажа в закрытую смену' : 'Новая продажа'}</h2>
      <div className="sale-entry-fields">
        <label className="field"><span>Дата</span><input required readOnly={!!closedShift} type="date" value={date} onChange={(e) => setDate(e.target.value)} /></label>
        <label className="field"><span>Время (МСК)</span><input required type="time" value={time} onChange={(e) => setTime(e.target.value)} /></label>
        <label className="field"><span>Кадры</span><input type="text" value={frames} maxLength={100} placeholder="9722–9716" onChange={(e) => setFrames(e.target.value)} /></label>
        <label className="field"><span>Тип заказа</span><select value={kind} onChange={(e) => setKind(e.target.value)}>
          {(ORDER_KINDS.includes(kind) ? ORDER_KINDS : [...ORDER_KINDS, kind]).map((x) => <option key={x}>{x}</option>)}
        </select></label>
        <label className="field"><span>Оплата</span><select disabled={!canEditAmounts || payments.loading} value={chosenPayment} onChange={(e) => setPayment(e.target.value)}>
          <option value="">Не выбрано</option>
          {chosenPayment && !methods.some((m) => m.id === chosenPayment) && <option value={chosenPayment}>Текущий способ</option>}
          {methods.map((m) => <option key={m.id} value={m.id}>{m.name}</option>)}
        </select></label>
        {sale && <label className="field"><span>Сотрудник</span>
          <select value={sellerId} disabled={staff.loading || !!staff.error} onChange={(e) => setSellerId(e.target.value)}>
            {!staff.data?.some((person) => person.id === sale.userId) && <option value={sale.userId}>{sale.userName}</option>}
            {staff.data?.map((person) => <option key={person.id} value={person.id}>{person.name}</option>)}
          </select>
        </label>}
        {closedShift && <label className="field"><span>Сотрудник</span>
          <select required value={sellerId} onChange={(e) => setSellerId(e.target.value)}>
            {shiftStaff.map((person) => <option key={person.userId} value={person.userId}>{person.name}</option>)}
          </select>
        </label>}
      </div>
      <label className="field"><span>Комментарий</span><textarea rows={2} value={comment} onChange={(e) => setComment(e.target.value)} /></label>
      {!sale && session.can('client.card') && <ClientPicker client={client} onChange={setClient} onPending={setClientPending} />}
      {!canEditAmounts && <p className="sub">Можно исправить сотрудника, время, кадры, комментарий и тип заказа. Сумма и состав связаны с расчётом чека.</p>}
      {canEditAmounts && <div className="sale-entry-fields">
        <label className="field"><span>Найти в каталоге</span><input type="search" value={term} onChange={(e) => setTerm(e.target.value)} /></label>
        <label className="field"><span>Добавить позицию</span><select value="" onChange={(e) => {
          const product = catalog.find((p) => p.id === e.target.value);
          if (!product) return;
          const price = product.stores.find((p) => p.storeId === storeId)?.salePrice ?? 0;
          setLines((current) => [...current, { key: crypto.randomUUID(), productId: product.id, name: product.name, qty: '1', price: String(price) }]);
          setChanged(true);
          setTerm('');
        }}>
          <option value="">{products.loading ? 'Загрузка…' : 'Выберите товар'}</option>
          {catalog.map((p) => <option key={p.id} value={p.id}>{p.name} · {rub(p.stores.find((price) => price.storeId === storeId)?.salePrice ?? 0)}</option>)}
        </select></label>
        {session.can('sale.custom_product') && <button type="button" className="btn light inline sale-entry-manual" onClick={() => {
          setLines((current) => [...current, { key: crypto.randomUUID(), productId: null, name: '', qty: '1', price: '' }]);
          setChanged(true);
        }}>Добавить вручную</button>}
      </div>}
      {(products.error || payments.error || staff.error || error) && <p className="error" role="alert">{error || staff.error || products.error || payments.error}</p>}
      {lines.map((line) => <div className="sale-entry-line" key={line.key}>
        <label className="field sale-entry-name"><span>Позиция</span>
          <input type="text" readOnly={!!line.productId || !canEditAmounts} required value={line.name}
            onChange={(e) => changeLine(line.key, { name: e.target.value })} />
        </label>
        <label className="field"><span>Кол-во</span>
          <input disabled={!canEditAmounts} required type="number" min="0.0001" step="0.0001" value={line.qty}
            onChange={(e) => changeLine(line.key, { qty: e.target.value })} />
        </label>
        <label className="field"><span>Цена, ₽</span>
          <input disabled={!canEditAmounts} required type="number" min="0" step="0.01" value={line.price}
            onChange={(e) => changeLine(line.key, { price: e.target.value })} />
        </label>
        {canEditAmounts && <button type="button" className="btn danger inline sale-entry-remove" aria-label={`Убрать ${line.name || 'позицию'}`} onClick={() => {
          setLines((current) => current.filter((item) => item.key !== line.key));
          setChanged(true);
        }}><span aria-hidden="true">×</span></button>}
      </div>)}
      <div className="row sale-entry-actions">
        <div><span className="label">Итого</span>
          <b>{rub(sale && !changed ? sale.total : lines.reduce((sum, line) => sum + Number(line.price) * Number(line.qty), sale?.deliveryPrice ?? 0))}</b>
        </div>
        <div className="row sale-entry-buttons">
          <button type="button" className="btn light inline" onClick={onCancel} disabled={busy}>Отмена</button>
          <button type="submit" className="btn inline" disabled={busy || clientPending || !lines.length || (!sale && payments.loading)}>{busy ? 'Сохранение…' : 'Сохранить'}</button>
        </div>
      </div>
    </form>
  );
}
