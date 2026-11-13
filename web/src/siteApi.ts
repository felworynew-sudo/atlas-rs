// Запросы к сайту студии (тот же домен, сессия дев-панели): картины и прочее, что ведётся на сайте.
export async function siteApi<T>(path: string, opts: { method?: string; body?: unknown } = {}): Promise<T> {
  const res = await fetch(path, {
    method: opts.method ?? (opts.body !== undefined ? 'POST' : 'GET'),
    credentials: 'same-origin',
    headers: opts.body !== undefined ? { 'Content-Type': 'application/json' } : undefined,
    body: opts.body !== undefined ? JSON.stringify(opts.body) : undefined,
  });
  const data = (await res.json().catch(() => null)) as (T & { ok?: boolean; error?: string }) | null;
  if (res.status === 401) throw new Error('Нужен вход в дев-панель сайта');
  if (!res.ok || !data || data.ok === false) throw new Error(data?.error || `Сайт ответил ${res.status}`);
  return data;
}

export interface Painting {
  row: number; date: string; type: string; size: string; telegram: string; phone: string; comment: string;
  payment: string; paymentStatus: string; disk: string; staff: string; status: string; tracker: string; trackerLink: string;
}
export interface PaintingOptions { type: string[]; size: string[]; paymentStatus: string[]; status: string[] }
