import { useState } from 'react';
import { useNavigate, useParams } from 'react-router-dom';
import { api, type Sale, type Shift } from '../api';
import { fmtDateTime } from '../dates';
import { rub } from '../money';
import { SaleEntry, saleDay, saleTime } from '../SaleEntry';
import { useSession } from '../session';
import { Header, Modal, Spinner, useLoad, useToast } from '../ui';

export default function ShiftOrders() {
  const { id = '' } = useParams();
  const nav = useNavigate();
  const session = useSession();
  const [entry, setEntry] = useState<Sale | 'new' | null>(null);
  const [toast, say] = useToast();
  const shift = useLoad(() => api<Shift>(`/shifts/${id}`), [id]);
  const sales = useLoad(() => api<Sale[]>('/sales', { query: { shiftId: id } }), [id]);
  const data = shift.data;
  const rows = sales.data ?? [];
  const canEdit = session.can('sale.new') && session.can('sale.history') && !!data && session.stores.some((s) => s.id === data.storeId);
  const canAdd = canEdit && (data!.closedAt ? data!.staff.some((person) => person.userId === session.user?.id) : true);
  // Смены старше недели правит только сотрудник с отдельным правом — предупреждаем заранее, а не после отказа.
  const closedDays = data?.closedAt ? Math.floor((Date.now() - new Date(data.closedAt).getTime()) / 86_400_000) : 0;
  const oldShift = closedDays >= 7;
  const oldShiftLocked = oldShift && !session.can('shift.edit_old');

  return <>
    <Header title="Заказы смены" />
    <main className="page wide shift-orders-page">
      <button className="link" onClick={() => nav('/shift')}>← История смен</button>
      {shift.loading && <Spinner />}
      {shift.error && <p className="error">{shift.error}</p>}
      {data && <>
        <div className="row wrap" style={{ alignItems: 'center', justifyContent: 'space-between', gap: 12 }}>
          <div><h1 className="h2" style={{ marginBottom: 4 }}>{fmtDateTime(data.openedAt)} — {data.closedAt ? fmtDateTime(data.closedAt) : 'открыта'}</h1><div className="sub">{data.openedByName} · {session.stores.find((s) => s.id === data.storeId)?.name}</div></div>
          {canAdd && !oldShiftLocked && <button className="btn inline" onClick={() => setEntry('new')}>Добавить заказ в эту смену</button>}
        </div>
        {oldShiftLocked && <p className="notice">Смена закрыта {closedDays} дн. назад — правка доступна только сотруднику с правом «Правка закрытых смен старше недели».</p>}
        <div className="row wrap" style={{ gap: 16 }}><span>Заказов <b>{data.totals.checks}</b></span><span>Продажи <b>{rub(data.totals.sales)}</b></span>{data.diff !== null && <span className="sub">Расхождение кассы {data.diff > 0 ? '+' : ''}{rub(data.diff)}</span>}</div>
        {sales.loading && <Spinner />}
        {sales.error && <p className="error">{sales.error}</p>}
        {!sales.loading && !sales.error && rows.length === 0 && <div className="empty">В этой смене нет заказов.</div>}
        <div className="shift-order-list">
          {rows.map((sale) => <article key={sale.id} className="card shift-order-card">
            <div className="row wrap" style={{ alignItems: 'start', justifyContent: 'space-between', gap: 8 }}><div><div className="title">{saleTime(new Date(sale.saleDate))} · {sale.items.map((i) => i.qty === 1 ? i.name : `${i.name} × ${i.qty}`).join(', ') || 'Заказ'}</div><div className="sub">{sale.userName}{sale.frameNumbers ? ` · кадры ${sale.frameNumbers}` : ''}{sale.comment ? ` · ${sale.comment}` : ''}</div></div><strong className="price">{rub(sale.total)}</strong></div>
            <div className="row wrap" style={{ marginTop: 10, gap: 8 }}><button className="btn light inline" onClick={() => nav(`/sale/${sale.id}`)}>Открыть чек</button>{canEdit && !oldShiftLocked && <button className="btn inline" onClick={() => setEntry(sale)}>Редактировать</button>}</div>
          </article>)}
        </div>
      </>}
    </main>
    {entry && data && <Modal className="shift-edit-modal" onClose={() => setEntry(null)}>
      <SaleEntry key={entry === 'new' ? 'new' : entry.id} storeId={data.storeId} day={saleDay(new Date(data.openedAt))} sale={entry === 'new' ? undefined : entry} closedShift={entry === 'new' && data.closedAt ? data : undefined}
        onCancel={() => setEntry(null)} onSaved={() => { setEntry(null); sales.reload(); shift.reload(); say('Заказ сохранён'); }} />
    </Modal>}
    {toast}
  </>;
}
