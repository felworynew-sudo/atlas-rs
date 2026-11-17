import { useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { api, type Shift, type StaffUser } from '../api';
import { fmtDateTime } from '../dates';
import { Icon } from '../Icon';
import { rub } from '../money';
import { Line } from '../reportKit';
import { SaleEntry, saleDay } from '../SaleEntry';
import { useSession } from '../session';
import { Header, Modal, Spinner, StoreChips, useLoad, useToast } from '../ui';

/** Смена: открытие с суммой в кассе, итоги по наличным и безналичным, внесение/изъятие, закрытие, история. */
/** Выгрузка закрытых смен в CSV (для Excel: разделитель «;», UTF-8 с BOM). */
function exportShifts(list: Shift[], storeName: string): void {
  const head = ['Филиал', 'Открыта', 'Закрыта', 'Касса на начало', 'Продажи', 'Чеков', 'Наличные', 'Безнал', 'Возвраты наличными', 'Расходы наличными', 'Приёмки', 'Внесения', 'Изъятия', 'Ожидалось', 'Факт', 'Разница'];
  const q = (v: string | number | null): string => `"${String(v ?? '').replace(/"/g, '""')}"`;
  const rows = list.map((x) => [storeName, fmtDateTime(x.openedAt), x.closedAt ? fmtDateTime(x.closedAt) : '', x.openingCash, x.totals.sales, x.totals.checks, x.totals.cashSales, x.totals.nonCashSales, x.totals.cashReturns, x.totals.cashSpendings, x.totals.paidSupplies, x.totals.deposits, x.totals.withdrawals, x.expectedCash, x.actualCash, x.diff]);
  const csv = String.fromCharCode(0xfeff) + [head, ...rows].map((r) => r.map(q).join(';')).join(String.fromCharCode(13, 10));
  const url = URL.createObjectURL(new Blob([csv], { type: 'text/csv;charset=utf-8' }));
  const a = document.createElement('a');
  a.href = url;
  a.download = 'shifts.csv';
  a.click();
  URL.revokeObjectURL(url);
}

export default function ShiftPage() {
  const s = useSession();
  const nav = useNavigate();
  const [toast, say] = useToast();
  const [store, setStore] = useState(s.storeId ?? '');
  const current = useLoad(() => api<Shift | null>('/shifts/current', { query: { storeId: store } }), [store]);
  const history = useLoad(() => api<Shift[]>('/shifts', { query: { storeId: store } }), [store]);
  const avatars = useLoad(() => fetch('/api/dev/staff', { credentials: 'same-origin' })
    .then((response) => response.ok ? response.json() as Promise<{ staff: { id: string; photo: string; photoMini?: string }[] }> : { staff: [] })
    .then((data) => data.staff), []);
  const [opening, setOpening] = useState('');
  const [modal, setModal] = useState<null | 'cashbox' | 'close' | 'staff' | 'opening'>(null);
  const [viewing, setViewing] = useState<Shift | null>(null);
  const [report, setReport] = useState<Shift | null>(null);
  const [closedAction, setClosedAction] = useState<'edit' | 'sale' | null>(null);
  const [error, setError] = useState<string | null>(null);

  const reloadAll = () => { current.reload(); history.reload(); };

  async function open() {
    setError(null);
    try {
      await api('/shifts/open', { body: { storeId: store, openingCash: Number(opening.replace(',', '.')) } });
      setOpening('');
      reloadAll();
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Ошибка');
    }
  }

  const sh = current.data;
  const closed = (history.data ?? []).filter((x) => x.closedAt);

  return (
    <>
      <Header title="Смена" />
      <main className="page">
        <StoreChips stores={s.stores} value={store} onChange={setStore} />
        {current.loading && <Spinner />}

        {!current.loading && !sh && (
          <>
            <div className="error" style={{ fontWeight: 600 }}>Смена закрыта</div>
            <div className="org-name" style={{ textAlign: 'left', fontSize: 28, margin: '4px 0 20px' }}>Откройте пожалуйста смену</div>
            <label className="field"><span>Наличные в кассе на начало смены</span>
              <input type="number" min="0.01" step="any" value={opening} placeholder="Введите сумму больше 0" onChange={(e) => setOpening(e.target.value)} />
            </label>
            {error && <div className="error" role="alert">{error}</div>}
            <button className="btn" disabled={!(Number(opening.replace(',', '.')) > 0)} onClick={open}>Открыть</button>
          </>
        )}

        {sh && (
          <>
            <div className="h2" style={{ marginTop: 16, fontSize: 18 }}>Сотрудники в смене</div>
            <div className="chips">
              {sh.staff.map((m) => (
                <span key={m.userId} className="chip active" style={{ display: 'inline-flex', alignItems: 'center', gap: 6 }}>
                  {m.name}
                  {s.can('shift.edit_staff') && (
                    <button aria-label={`Убрать ${m.name}`} style={{ background: 'none', border: 0, color: 'inherit', padding: 0 }} onClick={() => api(`/shifts/${sh.id}/staff/${m.userId}`, { method: 'DELETE' }).then(reloadAll).catch((e: unknown) => say(e instanceof Error ? e.message : 'Ошибка'))}>
                      <Icon name="close" size={14} />
                    </button>
                  )}
                </span>
              ))}
              {s.can('shift.edit_staff') && <button className="chip" onClick={() => setModal('staff')}>+ Добавить</button>}
            </div>

            <div className="h2" style={{ fontSize: 18 }}>Данные по смене</div>
            <div className="card">
              <Line label="Открытие смены" value={fmtDateTime(sh.openedAt)} />
              <div className="day">Наличные в кассе</div>
              <Line label="Сумма на начало смены" value={rub(sh.openingCash)} />
              {(s.can('shift.edit_data') || sh.openedBy === s.user?.id) && (
                <button className="link" onClick={() => setModal('opening')}>Исправить сумму на начало</button>
              )}
              <Line label="Продажи" value={rub(sh.totals.cashSales)} />
              <Line label="Возвраты" value={rub(sh.totals.cashReturns)} />
              <Line label="Изъятия" value={rub(sh.totals.withdrawals)} />
              <Line label="Расходы" value={rub(sh.totals.cashSpendings)} />
              <Line label="Оплаченные приемки" value={rub(sh.totals.paidSupplies)} />
              <Line label="Внесения" value={rub(sh.totals.deposits)} />
              <Line bold label="Итого" value={rub(sh.totals.expectedCash)} />
              {sh.totals.byMethod.filter((m) => m.kind !== 'cash').map((m) => (
                <div key={m.paymentMethodId}>
                  <div className="day">{m.name}</div>
                  <Line label="Продажи" value={rub(m.sales)} />
                  <Line label="Возвраты" value={rub(m.returns)} />
                </div>
              ))}
              <div className="day">Продажи по сотрудникам</div>
              {sh.totals.byUser.length === 0 && <div className="muted">Продаж пока нет</div>}
              {sh.totals.byUser.map((u) => <Line key={u.userId} label={`${u.name} (${u.checks})`} value={rub(u.sales)} />)}
            </div>
            <div style={{ display: 'grid', gap: 10, marginTop: 16 }}>
              <button className="btn light" onClick={() => setModal('cashbox')}>Изъятие / Внесение</button>
              {s.can('spending.view') && <button className="btn light" onClick={() => nav('/spending/new')}>Расход</button>}
              <button className="btn" onClick={() => setModal('close')}>Закрыть смену</button>
            </div>
          </>
        )}

        <div className="row" style={{ justifyContent: 'space-between', alignItems: 'center' }}>
          <div className="h2">История смен</div>
          {closed.length > 0 && <button className="link" onClick={() => exportShifts(closed, s.stores.find((x) => x.id === store)?.name ?? '')}>Отчет по сменам (CSV)</button>}
        </div>
        {closed.length === 0 && <div className="empty">Закрытых смен пока нет</div>}
        {closed.map((x) => (
          <div key={x.id} className="card click row" onClick={() => nav(`/shift/${x.id}/orders`)}>
            {(() => {
              const profile = avatars.data?.find((p) => p.id === x.openedByStaffId);
              const photo = profile?.photoMini || profile?.photo;
              return photo ? <img className="shift-avatar" src={photo} alt="" style={{ objectFit: 'contain', imageRendering: 'pixelated', background: 'transparent' }} />
                : <span className="shift-avatar" aria-hidden="true">{x.openedByName.trim().charAt(0).toUpperCase() || '•'}</span>;
            })()}
            <div className="grow">
              <div className="title">{fmtDateTime(x.openedAt)} — {fmtDateTime(x.closedAt as string)} · {x.openedByName}</div>
              <div className="sub">{x.diff === null ? 'Без сверки кассы' : `Расхождение кассы ${x.diff > 0 ? '+' : ''}${rub(x.diff)}`}{x.autoClosed ? ' · автозакрытие' : ''}</div>
            </div>
            <span className="price">{rub(x.totals.sales)}</span>
            <button type="button" className="btn light inline" onClick={(event) => { event.stopPropagation(); setViewing(x); }}>Отчёт</button>
          </div>
        ))}
      </main>

      {modal === 'cashbox' && sh && <CashboxModal shiftId={sh.id} onClose={() => setModal(null)} onDone={() => { setModal(null); reloadAll(); say('Успешно'); }} />}
      {modal === 'opening' && sh && <OpeningCashModal shift={sh} onClose={() => setModal(null)} onDone={() => { setModal(null); reloadAll(); say('Начальная наличка исправлена'); }} />}
      {modal === 'close' && sh && <CloseModal shift={sh} onClose={() => setModal(null)} onDone={(closed) => { setModal(null); reloadAll(); setReport(closed); }} />}
      {modal === 'staff' && sh && <StaffModal shift={sh} onClose={() => setModal(null)} onDone={() => { setModal(null); reloadAll(); }} />}
      {viewing && !closedAction && <ClosedModal shift={viewing} latest={closed[0]?.id === viewing.id}
        onEdit={() => setClosedAction('edit')} onSale={() => setClosedAction('sale')}
        onClose={() => setViewing(null)} onChanged={() => { setViewing(null); reloadAll(); }} />}
      {viewing && closedAction === 'edit' && <CloseModal shift={viewing} editing onClose={() => setClosedAction(null)}
        onDone={(updated) => { setViewing(updated); setClosedAction(null); reloadAll(); say('Смена обновлена'); }} />}
      {viewing && closedAction === 'sale' && <Modal onClose={() => setClosedAction(null)}>
        <SaleEntry storeId={viewing.storeId} day={saleDay(new Date(viewing.openedAt))} closedShift={viewing}
          onCancel={() => setClosedAction(null)} onSaved={() => {
            setClosedAction(null);
            setViewing(null);
            reloadAll();
            say('Продажа внесена в смену');
          }} />
      </Modal>}
      {report && <ReportModal shift={report} by={s.user?.name ?? ''} onClose={() => setReport(null)} />}
      {toast}
    </>
  );
}

function OpeningCashModal({ shift, onClose, onDone }: { shift: Shift; onClose: () => void; onDone: () => void }) {
  const [amount, setAmount] = useState(String(shift.openingCash));
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const value = Number(amount.replace(',', '.'));

  async function save() {
    setError(null);
    setBusy(true);
    try {
      await api(`/shifts/${shift.id}/opening-cash`, {
        method: 'PATCH',
        body: { revision: shift.revision, openingCash: value },
      });
      onDone();
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Ошибка');
    } finally {
      setBusy(false);
    }
  }

  return (
    <Modal onClose={onClose}>
      <div className="h2" style={{ marginTop: 0 }}>Наличные на начало смены</div>
      <label className="field"><span>Фактическая сумма в кассе при открытии</span>
        <input type="number" min="0" step="0.01" value={amount} onChange={(e) => setAmount(e.target.value)} />
      </label>
      {error && <div className="error" role="alert">{error}</div>}
      <button className="btn" disabled={busy || amount.trim() === '' || !Number.isFinite(value) || value < 0} onClick={save}>Сохранить</button>
    </Modal>
  );
}

function CashboxModal({ shiftId, onClose, onDone }: { shiftId: string; onClose: () => void; onDone: () => void }) {
  const [kind, setKind] = useState<'out' | 'in'>('out');
  const [amount, setAmount] = useState('');
  const [comment, setComment] = useState('');
  const [error, setError] = useState<string | null>(null);

  async function save() {
    setError(null);
    try {
      const v = Math.abs(Number(amount.replace(',', '.')));
      await api(`/shifts/${shiftId}/cashbox`, { body: { amount: kind === 'out' ? -v : v, comment } });
      onDone();
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Ошибка');
    }
  }

  return (
    <Modal onClose={onClose}>
      <div className="h2" style={{ marginTop: 0 }}>Изъятие / Внесение</div>
      <div className="chips">
        <button className={`chip${kind === 'out' ? ' active' : ''}`} onClick={() => setKind('out')}>Изъятие</button>
        <button className={`chip${kind === 'in' ? ' active' : ''}`} onClick={() => setKind('in')}>Внесение</button>
      </div>
      <label className="field"><span>Сумма</span><input type="number" min="0" step="any" value={amount} onChange={(e) => setAmount(e.target.value)} /></label>
      <label className="field"><span>Комментарий</span><input type="text" value={comment} onChange={(e) => setComment(e.target.value)} /></label>
      {error && <div className="error" role="alert">{error}</div>}
      <button className="btn" disabled={!(Number(amount) > 0)} onClick={save}>Сохранить</button>
    </Modal>
  );
}

/** «Закрытие смены»: фактическая сумма наличных и по каждому безналичному способу; недосдача считается сервером. */
function CloseModal({ shift, editing = false, onClose, onDone }: { shift: Shift; editing?: boolean; onClose: () => void; onDone: (closed: Shift) => void }) {
  const noncash = shift.totals.byMethod.filter((m) => m.kind !== 'cash');
  const [opening, setOpening] = useState(String(shift.openingCash));
  const [cash, setCash] = useState(String(editing ? shift.actualCash ?? '' : shift.totals.expectedCash));
  const [methods, setMethods] = useState<Record<string, string>>(Object.fromEntries(noncash.map((m) => [m.paymentMethodId, editing
    ? String(shift.closeDetails?.find((saved) => saved.paymentMethodId === m.paymentMethodId)?.actual ?? '')
    : String(m.sales - m.returns)])));
  const [comment, setComment] = useState(editing ? shift.comment : '');
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  async function save() {
    setError(null);
    setBusy(true);
    try {
      const url = editing ? `/shifts/${shift.id}` : `/shifts/${shift.id}/close`;
      const closed = await api<Shift>(url, {
        method: editing ? 'PATCH' : 'POST',
        body: {
          revision: editing ? shift.revision : undefined,
          openingCash: editing ? Number(opening.replace(',', '.')) : undefined,
          actualCash: Number(cash.replace(',', '.')),
          actualByMethod: noncash.filter((m) => methods[m.paymentMethodId] !== '').map((m) => ({
            paymentMethodId: m.paymentMethodId, amount: Number(methods[m.paymentMethodId].replace(',', '.')),
          })),
          comment,
        },
      });
      onDone(closed);
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Ошибка');
    } finally {
      setBusy(false);
    }
  }

  return (
    <Modal onClose={() => { if (!busy) onClose(); }}>
      <div className="h2" style={{ marginTop: 0 }}>{editing ? 'Правка закрытой смены' : 'Закрытие смены'}</div>
      {editing && <label className="field"><span>Наличные — сумма на начало дня</span>
        <input type="number" min="0" step="any" value={opening} onChange={(e) => setOpening(e.target.value)} />
      </label>}
      <label className="field"><span>Наличные — сумма на конец дня (по расчёту {rub(shift.totals.expectedCash)})</span>
        <input type="number" min="0" step="any" value={cash} onChange={(e) => setCash(e.target.value)} />
      </label>
      {noncash.map((m) => (
        <label key={m.paymentMethodId} className="field"><span>{m.name} — сумма на конец дня (по расчёту {rub(m.sales - m.returns)})</span>
          <input type="number" min="0" step="any" value={methods[m.paymentMethodId] ?? ''} onChange={(e) => setMethods({ ...methods, [m.paymentMethodId]: e.target.value })} />
        </label>
      ))}
      <label className="field"><span>Комментарий</span><input type="text" value={comment} onChange={(e) => setComment(e.target.value)} /></label>
      {error && <div className="error" role="alert">{error}</div>}
      <button className="btn" disabled={busy || cash === '' || (editing && opening === '')} onClick={save}>{editing ? 'Сохранить' : 'Закрыть'}</button>
    </Modal>
  );
}

/** Текст отчёта о закрытой смене: кто, когда, выручка, сколько в кассе, наличные и безнал (для экрана и для копирования в чат). */
function reportLines(shift: Shift, by: string): [string, string][] {
  const t = shift.totals;
  const who = shift.staff.map((m) => m.name).join(', ');
  const rows: [string, string][] = [
    ['Сотрудник', who || by || '—'],
    ['Открыта', fmtDateTime(shift.openedAt)],
    ['Закрыта', fmtDateTime(shift.closedAt as string)],
    ['Выручка', `${rub(t.sales)} (чеков ${t.checks})`],
    ['В кассе', rub(shift.actualCash ?? 0)],
    ['Наличными', rub(t.cashSales)],
    ['Безналом', rub(t.nonCashSales)],
  ];
  const noncash = t.byMethod.filter((m) => m.kind !== 'cash' && (m.sales !== 0 || m.returns !== 0));
  if (noncash.length > 1) for (const m of noncash) rows.push([`  ${m.name}`, rub(m.sales - m.returns)]);
  if ((shift.diff ?? 0) !== 0) rows.push([(shift.diff ?? 0) < 0 ? 'Недосдача' : 'Излишек', rub(shift.diff ?? 0)]);
  return rows;
}

/** После закрытия смены сразу показываем отчёт — его можно скопировать и отправить в чат. */
function ReportModal({ shift, by, onClose }: { shift: Shift; by: string; onClose: () => void }) {
  const rows = reportLines(shift, by);
  const [copied, setCopied] = useState(false);
  const copy = () => {
    const text = ['Смена закрыта', ...rows.map(([k, v]) => `${k}: ${v}`)].join(String.fromCharCode(10));
    navigator.clipboard.writeText(text).then(() => setCopied(true)).catch(() => undefined);
  };
  return (
    <Modal onClose={onClose}>
      <div className="h2" style={{ marginTop: 0 }}>Смена закрыта</div>
      <div className="card">
        {rows.map(([k, v], i) => <Line key={i} bold={k === 'В кассе'} label={k} value={v} tone={k === 'Недосдача' ? 'bad' : undefined} />)}
      </div>
      <div style={{ display: 'grid', gap: 10, marginTop: 16 }}>
        <button className="btn light" onClick={copy}>{copied ? 'Скопировано' : 'Скопировать отчёт'}</button>
        <button className="btn" onClick={onClose}>Готово</button>
      </div>
    </Modal>
  );
}

function StaffModal({ shift, onClose, onDone }: { shift: Shift; onClose: () => void; onDone: () => void }) {
  const users = useLoad(() => api<StaffUser[]>('/users').catch(() => [] as StaffUser[]), []);
  const free = (users.data ?? []).filter((u) => !shift.staff.some((m) => m.userId === u.id) && u.storeIds.includes(shift.storeId));
  return (
    <Modal onClose={onClose}>
      <div className="h2" style={{ marginTop: 0 }}>Выбор сотрудника</div>
      {users.loading && <Spinner />}
      {!users.loading && free.length === 0 && <div className="empty">Нет доступных сотрудников</div>}
      {free.map((u) => (
        <div key={u.id} className="card click" onClick={() => api(`/shifts/${shift.id}/staff`, { body: { userId: u.id } }).then(onDone).catch(() => undefined)}>
          <div className="title">{u.name}</div><div className="sub">{u.roleName}</div>
        </div>
      ))}
    </Modal>
  );
}

function ClosedModal({ shift, latest, onClose, onChanged, onEdit, onSale }: {
  shift: Shift; latest: boolean; onClose: () => void; onChanged: () => void; onEdit: () => void; onSale: () => void;
}) {
  const s = useSession();
  const [error, setError] = useState<string | null>(null);
  return (
    <Modal onClose={onClose}>
      <div className="h2" style={{ marginTop: 0 }}>Закрытая смена</div>
      <Line label="Открытие смены" value={fmtDateTime(shift.openedAt)} />
      <Line label="Закрытие смены" value={fmtDateTime(shift.closedAt as string)} />
      <Line label={`Продажи (${shift.totals.checks})`} value={rub(shift.totals.sales)} />
      {shift.totals.byUser.map((person) => <Line key={person.userId} label={`${person.name} (${person.checks})`} value={rub(person.sales)} />)}
      <div className="day">Наличные — расчёт и факт</div>
      <Line label="По расчёту" value={rub(shift.expectedCash ?? 0)} />
      <Line label="Факт" value={rub(shift.actualCash ?? 0)} />
      {(shift.closeDetails ?? []).map((d) => <Line key={d.paymentMethodId} label={`${d.name}: расчёт ${rub(d.expected)} / факт ${rub(d.actual)}`} value={rub(d.diff)} tone={d.diff < 0 ? 'bad' : undefined} />)}
      <Line bold label="Недосдача / излишек" value={rub(shift.diff ?? 0)} tone={(shift.diff ?? 0) < 0 ? 'bad' : undefined} />
      {shift.comment && <div className="muted" style={{ marginTop: 8 }}>{shift.comment}</div>}
      {error && <div className="error" role="alert">{error}</div>}
      {s.can('shift.edit_data') && <div className="row wrap" style={{ marginTop: 16 }}>
        <button className="btn inline" onClick={onEdit}>Исправить данные смены</button>
        {s.can('sale.new') && shift.staff.length > 0 && <button className="btn light inline" onClick={onSale}>Добавить продажу в смену</button>}
      </div>}
      {latest && s.can('shift.edit_data') && (
        <button className="btn danger" style={{ marginTop: 16 }} onClick={() => api(`/shifts/${shift.id}/reopen`, { method: 'POST' }).then(onChanged).catch((e: unknown) => setError(e instanceof Error ? e.message : 'Ошибка'))}>Отменить закрытие</button>
      )}
    </Modal>
  );
}
