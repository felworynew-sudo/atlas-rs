import type { ListItem } from './api';

const pixels = {
  cash: 'M0 2h12v8H0z M1 3v6h10V3z M2 4h2v1H2z M8 7h2v1H8z M5 4h2v4H5z',
  terminal: 'M1 2h10v8H1z M2 3v1h8V3z M2 6v3h8V6z M3 7h3v1H3z',
  transfer: 'M7 1h1v1h1v1h2v1H1V3h7V2H7z M5 11H4v-1H3V9H1V8h10v1H4v1h1z',
  cert: 'M1 5h10v6H1z M1 3h2V1h3v2h1V1h3v2h1v1H1z M5 5v6h2V5z',
  store: 'M1 1h10v2H1z M0 4h12v2H0z M1 7h10v4H1z M5 7v4h2V7z',
  delivery: 'M0 2h8v2h2v2h2v4h-1v1H9v-1H4v1H2v-1H0z M8 5v2h3V6h-2V5z M2 8v2h2V8z M9 8v2h2V8z',
  other: 'M1 5h2v2H1z M5 5h2v2H5z M9 5h2v2H9z',
};

export function paymentAppearance(method?: ListItem) {
  const name = method?.name.toLowerCase() ?? '';
  const kind = method?.params.kind;
  if (kind === 'cash' || /налич/.test(name)) return 'cash';
  if (kind === 'terminal' || /терминал|карт/.test(name)) return 'terminal';
  if (kind === 'transfer' || /перевод|сбп|qr/.test(name)) return 'transfer';
  if (kind === 'cert' || /сертификат/.test(name)) return 'cert';
  return 'other';
}

export function MethodPixelIcon({ method, purchase = false }: { method: ListItem; purchase?: boolean }) {
  const delivery = method.params.delivery === true || /достав/i.test(method.name);
  const icon = purchase ? (delivery ? 'delivery' : 'store') : paymentAppearance(method);
  return (
    <svg className={`method-icon method-icon-${icon}`} width="18" height="18" viewBox="0 0 12 12" fill="currentColor" aria-hidden="true" shapeRendering="crispEdges">
      <path d={pixels[icon]} fillRule="evenodd" />
    </svg>
  );
}
