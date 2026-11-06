// Даты в местном времени браузера: границы дней переводятся в UTC для сервера.
const pad = (n: number): string => String(n).padStart(2, '0');

export const todayLocal = (): string => {
  const d = new Date();
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
};

export const startOfDay = (day: string): string => new Date(`${day}T00:00:00`).toISOString();
export const endOfDay = (day: string): string => new Date(new Date(`${day}T00:00:00`).getTime() + 86_400_000).toISOString();

export const fmtDate = (iso: string): string => new Date(iso).toLocaleDateString('ru-RU');
export const fmtDateTime = (iso: string): string => new Date(iso).toLocaleString('ru-RU', { dateStyle: 'short', timeStyle: 'short' });

export const MOVEMENT_KINDS: Record<string, string> = {
  sale: 'Продажи',
  sale_deleted: 'Удалённые продажи',
  complex_sale: 'Комплексные продажи',
  writeoff: 'Списания',
  supply: 'Приемки',
  manufacture: 'Производства',
  transfer: 'Перемещения',
  return: 'Возвраты',
  revision: 'Ревизии',
  manual: 'Ручное изменение',
};

export const ORDER_STATUSES: Record<string, string> = {
  draft: 'Черновик',
  ordered: 'Заказан',
  partially_received: 'Получен частично',
  received: 'Получен',
  cancelled: 'Отменен',
};
