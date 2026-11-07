// Деньги и расчёт чека на клиенте (предпросмотр; итог всегда считает сервер).
const fmt = new Intl.NumberFormat('ru-RU', { maximumFractionDigits: 2 });

// «-0» (отрицательный ноль после округления) показываем как «0»
const clean = (v: number): number => (Math.abs(v) < 0.005 ? 0 : v);

// Валюта и число знаков организации (Настройки организации); по умолчанию рубли, до 2 знаков.
const SYMBOLS: Record<string, string> = { RUB: '₽', KZT: '₸', UAH: '₴', BYN: 'Br', USD: '$', EUR: '€', UZS: 'soʻm', KGS: 'с', GEL: '₾', AMD: '֏' };
let symbol = '₽';
export const CURRENCIES = Object.keys(SYMBOLS);
export function setCurrency(code: string, decimals: number): void {
  symbol = SYMBOLS[code] ?? code;
  fmtMoney = new Intl.NumberFormat('ru-RU', { maximumFractionDigits: Math.min(Math.max(decimals, 0), 4) });
}
let fmtMoney = fmt;

export const rub = (v: number): string => `${fmtMoney.format(clean(v))} ${symbol}`;
export const num = (v: number): string => fmt.format(clean(v));
export const round2 = (x: number): number => Math.round(x * 100) / 100;

export interface CartLine {
  key: string;
  productId?: string;
  name: string;
  price: number;
  qty: number;
}

export type Discount = { type: 'none' } | { type: 'percent'; value: number } | { type: 'amount'; value: number };

export interface Totals {
  subtotal: number;
  markup: number;
  discount: number;
  total: number;
}

/** Сумма позиций + надбавка метода покупки − скидка (как `domain::sale::totals` на сервере). */
export function saleTotals(lines: CartLine[], markupPercent: number, discount: Discount): Totals {
  const subtotal = round2(lines.reduce((s, l) => s + l.price * l.qty, 0));
  const markup = round2((subtotal * Math.max(markupPercent, -100)) / 100);
  let d = 0;
  if (discount.type === 'percent') d = round2((subtotal * Math.min(Math.max(discount.value, 0), 100)) / 100);
  if (discount.type === 'amount') d = Math.min(round2(Math.max(discount.value, 0)), subtotal);
  return { subtotal, markup, discount: d, total: round2(Math.max(subtotal + markup - d, 0)) };
}

/** Сдача с полученной суммы; null, если денег получено меньше, чем к оплате. */
export function change(received: number, total: number): number | null {
  return received + 1e-9 >= total ? round2(received - total) : null;
}
