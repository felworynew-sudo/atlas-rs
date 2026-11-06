// Клиент API Атласа: Bearer-токен сессии в localStorage, ошибки в формате {statusCode, message, error}.
const TOKEN_KEY = 'atlas.token';

export function getToken(): string | null {
  try {
    return localStorage.getItem(TOKEN_KEY);
  } catch {
    return null;
  }
}

export function setToken(token: string | null): void {
  try {
    if (token) localStorage.setItem(TOKEN_KEY, token);
    else localStorage.removeItem(TOKEN_KEY);
  } catch {
    /* хранилище недоступно — сессия живёт до перезагрузки */
  }
}

export class ApiError extends Error {
  constructor(public status: number, message: string) {
    super(message);
  }
}

type Query = Record<string, string | number | boolean | null | undefined>;

export function buildUrl(path: string, query?: Query): string {
  const params = new URLSearchParams();
  for (const [k, v] of Object.entries(query ?? {})) {
    if (v !== undefined && v !== null && v !== '') params.set(k, String(v));
  }
  const qs = params.toString();
  return `/api${path}${qs ? `?${qs}` : ''}`;
}

export async function api<T>(path: string, opts: { method?: string; body?: unknown; query?: Query } = {}): Promise<T> {
  const headers: Record<string, string> = {};
  const token = getToken();
  if (token) headers.Authorization = `Bearer ${token}`;
  if (opts.body !== undefined) headers['Content-Type'] = 'application/json';
  const res = await fetch(buildUrl(path, opts.query), {
    method: opts.method ?? (opts.body !== undefined ? 'POST' : 'GET'),
    headers,
    body: opts.body !== undefined ? JSON.stringify(opts.body) : undefined,
  });
  const text = await res.text();
  const data = text ? safeJson(text) : null;
  if (!res.ok) {
    if (res.status === 401) window.dispatchEvent(new Event('atlas:unauthorized'));
    const message = (data && typeof data === 'object' && 'message' in data ? String((data as { message: unknown }).message) : null) ?? `Ошибка ${res.status}`;
    throw new ApiError(res.status, message);
  }
  return data as T;
}

function safeJson(text: string): unknown {
  try {
    return JSON.parse(text);
  } catch {
    return null;
  }
}

/** Скачивание CSV-отчёта: запрос с токеном и сохранение файла из ответа. */
export async function downloadCsv(path: string, query: Query, filename: string): Promise<void> {
  const token = getToken();
  const res = await fetch(buildUrl(path, { ...query, format: 'csv' }), { headers: token ? { Authorization: `Bearer ${token}` } : {} });
  if (!res.ok) throw new ApiError(res.status, `Ошибка ${res.status}`);
  const url = URL.createObjectURL(await res.blob());
  const a = document.createElement('a');
  a.href = url;
  a.download = filename;
  a.click();
  URL.revokeObjectURL(url);
}

// ---- типы ответов сервера (только используемые поля) ----
export interface UserView {
  id: string;
  roleId?: string;
  organizationId: string;
  email: string;
  name: string;
  roleKey: string;
  roleName: string;
  permissions: string[];
  storeIds: string[];
}
export interface Org { id: string; name: string; modules: Record<string, boolean> }
export interface Store { id: string; name: string; address: string; timezone: string }
export interface ListItem { id: string; name: string; params: Record<string, unknown>; sortOrder: number }
export interface Category { id: string; name: string; sortOrder: number }
export interface StorePrice { storeId: string; active: boolean; salePrice: number }
export interface Product {
  id: string;
  name: string;
  isService: boolean;
  isComposite: boolean;
  hiddenInNewSale: boolean;
  purchasePrice: number;
  extraCost: number;
  costPrice: number;
  categoryId: string | null;
  supplierId: string | null;
  unitId: string | null;
  barcodes: string[];
  allowFractional: boolean;
  important: boolean;
  stockThreshold: number;
  stores: StorePrice[];
}
export interface StockRow {
  productId: string;
  name: string;
  categoryId: string | null;
  qty: number;
  threshold: number;
  low: boolean;
  sold7: number;
  writtenOff7: number;
}
export interface StockSummary { unitsInStock: number; valueAtPurchase: number; valueAtSale: number }
export interface Movement {
  id: string;
  kind: string;
  delta: number;
  qtyBefore: number;
  qtyAfter: number;
  refId: string | null;
  createdAt: string;
}
export interface Supplier { id: string; name: string }
export interface Wallet { id: string; name: string; storeId: string | null; isCashRegister: boolean; balance: number }
export interface SupplyItem { productId: string; name: string; qty: number; price: number }
export interface Supply {
  id: string;
  storeId: string;
  supplierId: string | null;
  orderId: string | null;
  supplyDate: string;
  hasReturn: boolean;
  pricesChanged: boolean;
  paid: boolean;
  comment: string;
  total: number;
  items: SupplyItem[];
}
export interface OrderItem { productId: string; name: string; qty: number; unitPrice: number }
export interface SupplyOrder {
  id: string;
  storeId: string;
  supplierId: string | null;
  status: string;
  orderDate: string;
  comment: string;
  total: number;
  items: OrderItem[];
}
export interface Suggestion { productId: string; name: string; currentQty: number; threshold: number; suggestedQty: number; unitPrice: number }
export interface Writeoff {
  id: string;
  storeId: string;
  productId: string;
  productName: string;
  qty: number;
  reasonName: string;
  amount: number;
  comment: string;
  writeoffDate: string;
}
export interface Spending {
  id: string;
  storeId: string;
  categoryId: string | null;
  categoryName: string | null;
  details: string;
  amount: number;
  walletId: string | null;
  spendingDate: string;
  kind: 'operating' | 'capital';
}
export interface SalesGroup { id: string | null; name: string; sales: number; count: number; returns: number; returnsCount: number }
export interface SalesReport {
  totals: { sales: number; count: number; avgCheck: number; returns: number; returnsCount: number };
  groups: SalesGroup[];
  days: { date: string; sales: number; count: number }[];
}
export interface ProfitReport {
  sales: number;
  costOfSales: number;
  paidByBonus: number;
  grossIncome: number;
  returns: number;
  writeoffs: number;
  revisionDiff: number;
  shiftDiff: number;
  salaries: number;
  bankCommission: number;
  expenses: { total: number; byCategory: { name: string; amount: number }[] };
  monthlyExpenses: { total: number; items: { name: string; amount: number }[] };
  operatingProfit: number;
  capitalExpenses: number;
  netProfit: number;
  supplies: number;
  checks: number;
}
export interface ProductAgg {
  id: string | null;
  name: string;
  soldQty: number;
  soldSum: number;
  cost: number;
  profit: number;
  margin: number;
  checks: number;
  writeoffQty: number;
  writeoffSum: number;
  supplyQty: number;
  supplySum: number;
}
export interface ProductsReport { byCategory: ProductAgg[]; bySupplier: ProductAgg[]; byProduct: ProductAgg[] }
export interface RevisionLine { name: string; was: number; became: number; diffAtPurchase: number; diffAtSale: number }
export interface RevisionsReport { minus: number; plus: number; balance: number; minusList: RevisionLine[]; plusList: RevisionLine[] }
export interface RevisionItem { productId: string; name: string; systemQty: number; factQty: number; diffAmount: number }
export interface Revision { id: string; storeId: string; categoryId: string | null; totalDiff: number; revisionDate: string; comment: string; items: RevisionItem[] }
export interface Client {
  id: string;
  name: string;
  phone: string;
  address: string;
  clientType: 'person' | 'organization';
  discountId: string | null;
  comment: string;
  bonusBalance: number;
  bonusLevel: number | null;
  punches: number;
  purchases: number;
  purchasesSum: number;
  lastPurchaseAt: string | null;
  createdAt: string;
}
export interface Purchase { id: string; storeId: string; saleDate: string; total: number; returnedAmount: number }
export interface Role {
  id: string;
  key: string;
  name: string;
  salaryType: 'none' | 'hour' | 'day' | 'month';
  salaryRate: number;
  bonusType: 'none' | 'percent_own_sales';
  bonusRate: number;
  permissions: string[];
}
export interface PermissionDef { key: string; title: string; module: string | null }
export interface StaffUser { id: string; email: string; name: string; roleId: string; roleKey: string; roleName: string; storeIds: string[] }
export interface SaleItem { id: string; productId: string | null; name: string; qty: number; price: number; returnedQty: number }
export interface Sale {
  id: string;
  storeId: string;
  userName: string;
  clientId: string | null;
  paymentMethodId: string | null;
  purchaseMethodId: string | null;
  subtotal: number;
  discountAmount: number;
  total: number;
  returnedAmount: number;
  saleDate: string;
  items: SaleItem[];
}
export interface SavedCart { id: string; storeId: string; name: string; payload: { lines?: unknown[]; clientId?: string | null } }
export interface HomeStats { today: { sales: number; checks: number; avgCheck: number }; weekAgo: { sales: number; checks: number; avgCheck: number } }

export interface MethodTotals { paymentMethodId: string; name: string; kind: string; sales: number; returns: number }
export interface ShiftTotals {
  sales: number;
  checks: number;
  cashSales: number;
  nonCashSales: number;
  cashReturns: number;
  cashSpendings: number;
  paidSupplies: number;
  deposits: number;
  withdrawals: number;
  expectedCash: number;
  byMethod: MethodTotals[];
  byUser: { userId: string; name: string; sales: number; checks: number }[];
}
export interface Shift {
  id: string;
  storeId: string;
  openedAt: string;
  closedAt: string | null;
  openingCash: number;
  expectedCash: number | null;
  actualCash: number | null;
  diff: number | null;
  comment: string;
  autoClosed: boolean;
  closeDetails: { paymentMethodId: string; name: string; expected: number; actual: number; diff: number }[] | null;
  staff: { userId: string; name: string }[];
  cashbox: { id: string; amount: number; comment: string; createdAt: string }[];
  totals: ShiftTotals;
}
export interface SalaryRow {
  userId: string;
  name: string;
  roleName: string;
  salaryType: string;
  rate: number;
  bonusType: string;
  bonusRate: number;
  hours: number;
  shifts: number;
  base: number;
  bonus: number;
  accrued: number;
  paid: number;
  toPay: number;
}
export interface SalaryDetail extends SalaryRow {
  balancePeriod: number;
  balanceAll: number;
  workingHours: { id: string; storeId: string; startedAt: string; endedAt: string | null }[];
  payments: { id: string; amount: number; walletId: string | null; comment: string; paidOn: string }[];
}
export interface ChecklistItem { id: string; title: string; time: string; sortOrder: number; kind: 'daily' | 'once' | 'section'; done: boolean; doneAt: string | null; doneBy: string | null }

export interface ComplexItem { id?: string; productId: string | null; name: string; qty: number; cost: number; extraCost: number }
export interface ComplexSale {
  id: string;
  storeId: string;
  clientId: string | null;
  templateName: string;
  name: string;
  status: 'new' | 'in_progress' | 'done' | 'cancelled';
  comment: string;
  finalPrice: number;
  saleDate: string;
  expensesTotal: number;
  paid: number;
  expectedProfit: number;
  remaining: number;
  items: ComplexItem[];
  payments: { id: string; walletId: string | null; amount: number; comment: string; paidAt: string }[];
  updates: { kind: string; text: string; createdAt: string }[];
}
export interface ComplexTemplate { id: string; name: string; finalPrice: number; items: { name: string; qty?: number; cost?: number; extraCost?: number; productId?: string | null }[] }
export interface ClientOrder {
  id: string;
  number: number;
  storeId: string;
  source: string;
  clientName: string;
  clientPhone: string;
  items: { productId: string | null; name: string; qty: number; price: number }[];
  total: number;
  status: 'new' | 'in_work' | 'ready' | 'done' | 'cancelled';
  paymentMethod: string;
  deliveryAddress: string;
  notes: string;
  saleId: string | null;
  createdAt: string;
}
