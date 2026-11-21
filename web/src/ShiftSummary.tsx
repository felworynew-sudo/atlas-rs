import { useState } from 'react';
import { api, type Shift } from './api';
import { fmtDateTime } from './dates';
import { rub } from './money';
import { Line } from './reportKit';
import { Modal } from './ui';

/** Движения по кассе: изъятие (минус) и внесение (плюс). */
function cashboxLines(shift: Shift): [string, number][] {
  return shift.cashbox.map((item) => [item.comment?.trim() || 'Без комментария', item.amount]);
}

/**
 * Итоги смены одним взглядом: сколько пришло, сколько должно быть в кассе и сколько там
 * фактически. Безналичные способы — отдельным блоком, нулевые строки не показываются:
 * в предыдущей версии «Наличные — расчёт и факт» содержало под собой терминал, перевод
 * и сертификат, а нули занимали полэкрана.
 */
export function ShiftSummary({ shift, onEditOpening, canEditOpening }: {
  shift: Shift;
  onEditOpening?: () => void;
  canEditOpening?: boolean;
}) {
  const [details, setDetails] = useState(false);
  const t = shift.totals;
  const closed = !!shift.closedAt;
  const diff = shift.diff ?? 0;
  // Разделяем расхождение по кассе и по безналичным способам: иначе ошибка в терминале
  // выглядит как «недосдача наличных», хотя касса сошлась (реальный случай 21.11.2026).
  const cashDiff = Math.round(((shift.actualCash ?? 0) - t.expectedCash) * 100) / 100;
  const closeDetails = shift.closeDetails ?? [];
  const methodDiff = (id: string) => closeDetails.find((d) => d.paymentMethodId === id);
  const noncash = t.byMethod.filter((m) => m.kind !== 'cash' && (m.sales !== 0 || m.returns !== 0));
  const movements = cashboxLines(shift);
  const nonCashTotal = t.nonCashSales - noncash.reduce((sum, m) => sum + m.returns, 0);
  // Расход наличными, приёмки и возвраты показываем, только если они были.
  const extras: [string, number, string][] = [
    ['Возвраты наличными', t.cashReturns, '−'],
    ['Расходы наличными', t.cashSpendings, '−'],
    ['Оплаченные приёмки', t.paidSupplies, '−'],
    ['Внесения', t.deposits, '+'],
    ['Изъятия', t.withdrawals, '−'],
  ];

  return <>
    <div className="card">
      <div className="day" style={{ marginTop: 0 }}>Выручка</div>
      <Line bold label={`Всего${t.checks > 1 ? ` · чеков ${t.checks}` : ''}`} value={rub(t.sales)} />
      <Line label="Наличными" value={rub(t.cashSales)} />
      {nonCashTotal !== 0 && <Line label="Безналом" value={rub(nonCashTotal)} />}
    </div>

    <div className="card">
      <div className="day" style={{ marginTop: 0 }}>Касса</div>
      <div className="row" style={{ justifyContent: 'space-between', gap: 12, padding: '6px 0' }}>
        <span>На начало смены</span>
        <span className="row" style={{ gap: 10 }}>
          <span>{rub(shift.openingCash)}</span>
          {onEditOpening && canEditOpening && <button className="btn light inline" onClick={onEditOpening}>Изменить</button>}
        </span>
      </div>
      <Line label="Пришло наличными" value={rub(t.cashSales)} />
      {extras.filter(([, amount]) => amount !== 0).map(([label, amount, sign]) => (
        <Line key={label} label={label} value={`${sign}${rub(Math.abs(amount))}`} />
      ))}
      <Line bold label={closed ? 'Должно было быть' : 'Должно быть сейчас'} value={rub(t.expectedCash)} />
      {closed && <>
        <Line bold label="Фактически сдали" value={rub(shift.actualCash ?? 0)} />
        {cashDiff !== 0 && <Line bold label={cashDiff < 0 ? 'Недосдача по кассе' : 'Излишек по кассе'} value={rub(Math.abs(cashDiff))} tone={cashDiff < 0 ? 'bad' : 'good'} />}
        {diff !== 0 && cashDiff === 0 && <Line label="Расхождение по кассе" value="нет" tone="good" />}
        {diff !== 0 && <Line label="Итого расхождение" value={`${diff < 0 ? '−' : '+'}${rub(Math.abs(diff))}`} tone={diff < 0 ? 'bad' : 'good'} />}
        {diff === 0 && <Line label="Расхождение" value="нет" tone="good" />}
      </>}
    </div>

    {movements.length > 0 && <div className="card">
      <div className="day" style={{ marginTop: 0 }}>Инкассация</div>
      {movements.map(([comment, amount], i) => (
        <Line key={i} label={`${amount < 0 ? 'Убрали из кассы' : 'Положили в кассу'} · ${comment}`}
          value={`${amount < 0 ? '−' : '+'}${rub(Math.abs(amount))}`} tone={amount < 0 ? 'bad' : 'good'} />
      ))}
    </div>}

    {noncash.length > 0 && <div className="card">
      <div className="day" style={{ marginTop: 0 }}>Безналичные оплаты</div>
      {noncash.map((m) => {
        const d = methodDiff(m.paymentMethodId);
        const expected = d ? d.expected : m.sales - m.returns;
        const actual = d ? d.actual : null;
        const bad = d ? Math.abs(d.diff) > 0.005 : false;
        return <Line key={m.paymentMethodId}
          label={d ? `${m.name} · факт ${rub(actual ?? 0)} (расчёт ${rub(expected)})` : m.name}
          value={d && bad ? `${d.diff < 0 ? '−' : '+'}${rub(Math.abs(d.diff))}` : rub(expected)}
          tone={bad ? (d!.diff < 0 ? 'bad' : 'good') : undefined} />;
      })}
    </div>}

    <button className="link" onClick={() => setDetails((on) => !on)}>{details ? 'Скрыть детали' : 'Показать детали'}</button>

    {details && <>
      <div className="card">
        <div className="day" style={{ marginTop: 0 }}>Когда</div>
        <Line label="Открыта" value={fmtDateTime(shift.openedAt)} />
        <Line label="Закрыта" value={closed ? fmtDateTime(shift.closedAt as string) : 'ещё идёт'} />
        {shift.autoClosed && <Line label="Закрыта автоматически" value="да" />}
      </div>
      <div className="card">
        <div className="day" style={{ marginTop: 0 }}>Кто в смене</div>
        {shift.staff.length === 0 && <div className="muted">Сотрудники не указаны</div>}
        {t.byUser.map((person) => (
          <Line key={person.userId} label={person.checks > 1 ? `${person.name} · чеков ${person.checks}` : person.name} value={rub(person.sales)} />
        ))}
      </div>
      {closed && (shift.closeDetails ?? []).length > 0 && <div className="card">
        <div className="day" style={{ marginTop: 0 }}>Сверка по способам оплаты</div>
        {(shift.closeDetails ?? []).map((d) => (
          <Line key={d.paymentMethodId} label={`${d.name} · расчёт ${rub(d.expected)}`} value={`факт ${rub(d.actual)}`} />
        ))}
      </div>}
      {shift.comment && <div className="card"><div className="day" style={{ marginTop: 0 }}>Комментарий</div><div>{shift.comment}</div></div>}
    </>}
  </>;
}

/** Правка наличных на начало смены: было спрятано под мелкой ссылкой, из-за чего её не находили. */
export function OpeningCashModal({ shift, onClose, onDone }: { shift: Shift; onClose: () => void; onDone: () => void }) {
  const [amount, setAmount] = useState(String(shift.openingCash));
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const value = Number(amount.replace(',', '.'));
  return (
    <Modal onClose={onClose}>
      <div className="h2" style={{ marginTop: 0 }}>Наличные на начало смены</div>
      <p className="muted" style={{ marginTop: 0 }}>Сколько было в кассе, когда смену открыли. Сумму можно поправить, пока смена идёт.</p>
      <label className="field"><span>Сумма в кассе при открытии, ₽</span>
        <input type="number" min="0" step="0.01" autoFocus value={amount} onChange={(e) => setAmount(e.target.value)} />
      </label>
      {error && <div className="error" role="alert">{error}</div>}
      <button className="btn" disabled={busy || amount.trim() === '' || !Number.isFinite(value) || value < 0} onClick={async () => {
        setError(null);
        setBusy(true);
        try {
          await api(`/shifts/${shift.id}/opening-cash`, { method: 'PATCH', body: { revision: shift.revision, openingCash: value } });
          onDone();
        } catch (e) {
          setError(e instanceof Error ? e.message : 'Ошибка');
        } finally {
          setBusy(false);
        }
      }}>Сохранить</button>
    </Modal>
  );
}
