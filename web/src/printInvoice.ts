import { fmtDate } from './dates';
import { num, rub } from './money';

export interface PrintTemplate {
  id: string;
  name: string;
  params: {
    storeId?: string;
    title?: string;
    subtitle?: string;
    pageSize?: string;
    margin?: number;
    fields?: Record<string, boolean>;
    signLeft?: { on?: boolean; text?: string };
    signRight?: { on?: boolean; text?: string };
    footer?: string;
  };
}

export const INVOICE_FIELDS: [string, string][] = [
  ['number', 'Номер накладной'],
  ['date', 'Дата'],
  ['supplier', 'Поставщик'],
  ['table', 'Таблица товаров'],
  ['total', 'Общая сумма'],
  ['words', 'Сумма прописью'],
  ['vat', 'Включать НДС'],
];

const esc = (t: string): string => t.replace(/[&<>"]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' })[c] as string);

const UNITS_M = ['', 'один', 'два', 'три', 'четыре', 'пять', 'шесть', 'семь', 'восемь', 'девять'];
const UNITS_F = ['', 'одна', 'две', 'три', 'четыре', 'пять', 'шесть', 'семь', 'восемь', 'девять'];
const TEENS = ['десять', 'одиннадцать', 'двенадцать', 'тринадцать', 'четырнадцать', 'пятнадцать', 'шестнадцать', 'семнадцать', 'восемнадцать', 'девятнадцать'];
const TENS = ['', '', 'двадцать', 'тридцать', 'сорок', 'пятьдесят', 'шестьдесят', 'семьдесят', 'восемьдесят', 'девяносто'];
const HUND = ['', 'сто', 'двести', 'триста', 'четыреста', 'пятьсот', 'шестьсот', 'семьсот', 'восемьсот', 'девятьсот'];

function triple(n: number, female: boolean): string {
  const h = Math.floor(n / 100);
  const t = Math.floor((n % 100) / 10);
  const u = n % 10;
  const out = [HUND[h]];
  if (t === 1) out.push(TEENS[u]);
  else out.push(TENS[t], (female ? UNITS_F : UNITS_M)[u]);
  return out.filter(Boolean).join(' ');
}

const plural = (n: number, forms: [string, string, string]): string => {
  const m = n % 100;
  if (m >= 11 && m <= 14) return forms[2];
  const d = n % 10;
  return d === 1 ? forms[0] : d >= 2 && d <= 4 ? forms[1] : forms[2];
};

/** Сумма прописью в рублях и копейках. */
export function amountInWords(amount: number): string {
  const rubles = Math.floor(amount + 1e-9);
  const kop = Math.round((amount - rubles) * 100);
  const parts: string[] = [];
  const groups: [number, boolean, [string, string, string] | null][] = [
    [Math.floor(rubles / 1_000_000_000) % 1000, false, ['миллиард', 'миллиарда', 'миллиардов']],
    [Math.floor(rubles / 1_000_000) % 1000, false, ['миллион', 'миллиона', 'миллионов']],
    [Math.floor(rubles / 1000) % 1000, true, ['тысяча', 'тысячи', 'тысяч']],
    [rubles % 1000, false, null],
  ];
  for (const [g, female, forms] of groups) {
    if (g === 0) continue;
    parts.push(triple(g, female));
    if (forms) parts.push(plural(g, forms));
  }
  const words = rubles === 0 ? 'ноль' : parts.join(' ');
  const text = `${words} ${plural(rubles, ['рубль', 'рубля', 'рублей'])} ${String(kop).padStart(2, '0')} ${plural(kop, ['копейка', 'копейки', 'копеек'])}`;
  return text.charAt(0).toUpperCase() + text.slice(1);
}

export interface InvoiceData {
  number: string;
  date: string;
  supplier: string;
  items: { name: string; qty: number; price: number }[];
  total: number;
}

/** HTML накладной по шаблону: включённые поля, подписи, дополнительный текст. */
export function invoiceHtml(t: PrintTemplate['params'], d: InvoiceData): string {
  const f = t.fields ?? {};
  const on = (k: string): boolean => f[k] !== false;
  const vat = f.vat === true;
  const rows = d.items
    .map((i, k) => `<tr><td>${k + 1}</td><td>${esc(i.name)}</td><td class="r">${esc(num(i.qty))}</td><td class="r">${esc(rub(i.price))}</td><td class="r">${esc(rub(i.qty * i.price))}</td></tr>`)
    .join('');
  const sign = (s?: { on?: boolean; text?: string }): string => (s?.on ? `<div class="sign">${esc(s.text ?? '')}<span></span></div>` : '');
  return `<!doctype html><meta charset="utf-8"><style>
body{font-family:sans-serif;margin:0;padding:${t.margin ?? 10}px;font-size:13px}
h1{font-size:20px;margin:0 0 4px}h2{font-size:14px;font-weight:normal;margin:0 0 12px;color:#444}
table{border-collapse:collapse;width:100%;margin:12px 0}td,th{border:1px solid #999;padding:4px 6px;text-align:left}.r{text-align:right}
.signs{display:flex;justify-content:space-between;margin-top:40px}.sign span{display:inline-block;border-bottom:1px solid #000;width:160px;margin-left:8px}
@page{size:${esc(t.pageSize || 'A4')}}</style>
<h1>${esc(t.title || 'Накладная')}</h1>${t.subtitle ? `<h2>${esc(t.subtitle)}</h2>` : ''}
${on('number') ? `<div>№ ${esc(d.number)}</div>` : ''}${on('date') ? `<div>Дата: ${esc(fmtDate(d.date))}</div>` : ''}${on('supplier') ? `<div>Поставщик: ${esc(d.supplier)}</div>` : ''}
${on('table') ? `<table><thead><tr><th>№</th><th>Товар</th><th class="r">Кол-во</th><th class="r">Цена</th><th class="r">Сумма</th></tr></thead><tbody>${rows}</tbody></table>` : ''}
${on('total') ? `<div><b>Итого: ${esc(rub(d.total))}</b></div>` : ''}${vat ? `<div>В том числе НДС 20%: ${esc(rub((d.total * 20) / 120))}</div>` : ''}
${on('words') ? `<div>${esc(amountInWords(d.total))}</div>` : ''}
${t.footer ? `<p>${esc(t.footer)}</p>` : ''}
<div class="signs"><div>${sign(t.signLeft)}</div><div>${sign(t.signRight)}</div></div>
<script>onload=()=>print()</script>`;
}

export function printInvoice(t: PrintTemplate['params'], d: InvoiceData): boolean {
  const w = window.open('', '_blank');
  if (!w) return false;
  w.document.write(invoiceHtml(t, d));
  w.document.close();
  return true;
}
