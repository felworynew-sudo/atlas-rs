import { useState, type ReactNode } from 'react';
import { downloadCsv } from './api';
import { useSession } from './session';
import { DateRange, StoreChips, useToast } from './ui';

/** Общие фильтры отчётов: филиал («Все» или один) и период; query — параметры для `/api/reports/*`. */
export function useReportFilters(defaultFrom: string, defaultTo: string, allStores = true) {
  const s = useSession();
  const [store, setStore] = useState<string>(allStores ? 'all' : s.storeId ?? '');
  const [from, setFrom] = useState(defaultFrom);
  const [to, setTo] = useState(defaultTo);
  // период датами: сервер сам считает границы суток по часовому поясу филиала
  const query = { storeId: store === 'all' ? undefined : store, fromDate: from, toDate: to };
  const ui: ReactNode = (
    <>
      <StoreChips all={allStores} stores={s.stores} value={store} onChange={setStore} />
      <DateRange from={from} to={to} onChange={(a, b) => { setFrom(a); setTo(b); }} />
    </>
  );
  return { store, from, to, query, ui };
}

/** Кнопка «Экспорт в CSV» для отчёта. */
export function CsvButton({ path, query, file }: { path: string; query: Record<string, string | number | boolean | null | undefined>; file: string }) {
  const [toast, say] = useToast();
  return (
    <>
      <button className="link" onClick={() => downloadCsv(path, query, file).catch((e: unknown) => say(e instanceof Error ? e.message : 'Ошибка'))}>Экспорт в CSV</button>
      {toast}
    </>
  );
}

/** Строка «название — сумма» таблицы показателей. */
export function Line({ label, value, bold, tone, indent }: { label: string; value: string; bold?: boolean; tone?: 'good' | 'bad'; indent?: boolean }) {
  return (
    <div className="row" style={{ justifyContent: 'space-between', padding: '6px 0', paddingLeft: indent ? 16 : 0, fontWeight: bold ? 800 : 400, color: tone === 'good' ? 'var(--success)' : tone === 'bad' ? 'var(--danger)' : undefined }}>
      <span>{label}</span>
      <span>{value}</span>
    </div>
  );
}
