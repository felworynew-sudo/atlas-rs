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

// ---- типы ответов сервера (только используемые поля) ----
export interface UserView {
  id: string;
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
export interface RevisionItem { productId: string; name: string; systemQty: number; factQty: number; diffAmount: number }
export interface Revision { id: string; storeId: string; categoryId: string | null; totalDiff: number; revisionDate: string; comment: string; items: RevisionItem[] }
export interface Client { id: string; name: string; phone: string; bonusBalance: number; bonusLevel: number | null; punches: number }
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
