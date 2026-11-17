import { useMemo, useRef, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { api, ApiError, type PriceType, type Category, type Client, type ListItem, type Product, type Sale, type SavedCart, type StockRow } from '../api';
import { Icon } from '../Icon';
import { MethodPixelIcon, paymentAppearance } from '../MethodPixelIcon';
import { ClientPicker, normalizedPhone } from '../ClientPicker';
import { change, num, rub, saleTotals, type CartLine, type Discount } from '../money';
import { useSession } from '../session';
import { Header, Modal, Spinner, useLoad, useMedia, useToast } from '../ui';
import { PaintingForm, guessPainting, type PaintingDraft } from '../PaintingForm';
import { emptyPainting, staffLabel } from './Paintings';

const BILLS = [1, 5, 10, 50, 100, 500, 1000, 5000];

export default function NewSale() {
  const s = useSession();
  const nav = useNavigate();
  const wide = useMedia('(min-width: 718px)');
  const [toast, say] = useToast();
  const storeId = s.storeId ?? '';

  const products = useLoad(() => api<Product[]>('/products', { query: { storeId } }), [storeId]);
  const stock = useLoad(() => (s.can('stock.view') ? api<StockRow[]>('/stocks', { query: { storeId } }) : Promise.resolve([] as StockRow[])), [storeId]);
  const cats = useLoad(() => api<Category[]>('/product-categories'), []);
  const payments = useLoad(() => api<ListItem[]>('/lists/payment_methods'), []);
  const purchases = useLoad(() => api<ListItem[]>('/lists/purchase_methods'), []);
  const discounts = useLoad(() => api<ListItem[]>('/lists/discounts'), []);
  const carts = useLoad(() => api<SavedCart[]>('/saved-carts', { query: { storeId } }), [storeId]);

  const sset = useLoad(() => api<Record<string, unknown>>('/sale-settings'), []);
  const flag = (k: string, d: boolean): boolean => (typeof sset.data?.[k] === 'boolean' ? (sset.data[k] as boolean) : d);
  const [saleComment, setSaleComment] = useState('');
  const [saleDate, setSaleDate] = useState('');
  const [tableNo, setTableNo] = useState('');
  const modifiers = useLoad(() => api<ListItem[]>('/lists/product_modifiers'), []);
  const priceTypes = useLoad(() => api<PriceType[]>('/price-types'), []);
  const [rawLines, setLines] = useState<CartLine[]>([]);
  const [typeChoice, setTypeChoice] = useState<string>(''); // '' — авто, 'base' — базовая, иначе id типа цены
  const [search, setSearch] = useState('');
  const [cat, setCat] = useState<string>('all');
  const [catalogOpen, setCatalogOpen] = useState(false);
  const [client, setClient] = useState<Client | null>(null);
  const [clientPending, setClientPending] = useState(false);
  const [clientDraft, setClientDraft] = useState({ phone: '', name: '' });
  const [discount, setDiscount] = useState<Discount>({ type: 'none' });
  const [purchaseId, setPurchaseId] = useState<string | null>(null);
  const [paymentId, setPaymentId] = useState<string | null>(null);
  const [mixed, setMixed] = useState(false);
  const [secondId, setSecondId] = useState<string | null>(null);
  const [firstValue, setFirstValue] = useState('');
  const [received, setReceived] = useState('');
  const [bonusInput, setBonusInput] = useState('');
  const [cashbackOn, setCashbackOn] = useState(true);
  const [deliveryOn, setDeliveryOn] = useState(false);
  const [deliveryAddress, setDeliveryAddress] = useState('');
  const [deliveryPhone, setDeliveryPhone] = useState('');
  const [modal, setModal] = useState<null | 'discount' | 'custom' | 'carts' | 'mods' | 'unavailable' | 'opening-shift'>(null);
  const [openingCash, setOpeningCash] = useState('');
  const [openingError, setOpeningError] = useState('');
  const pendingCheckout = useRef(false);
  const [custom, setCustom] = useState({ name: '', price: '' });
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [done, setDone] = useState<Sale | null>(null);
  // «Это картина»: после продажи сразу карточка картины для листа «Печать» (тип и размер — по названиям товаров)
  const [isPainting, setIsPainting] = useState(false);
  const [painting, setPainting] = useState<PaintingDraft | null>(null);
  const idem = useRef(crypto.randomUUID());

  const stockMap = useMemo(() => new Map((stock.data ?? []).map((r) => [r.productId, r])), [stock.data]);
  const purchase = (purchases.data ?? []).find((p) => p.id === (purchaseId ?? purchases.data?.[0]?.id));
  // по умолчанию — терминал (так платят чаще всего); если терминала нет — первый способ из списка
  const defaultPaymentId = ((payments.data ?? []).find((p) => p.params.kind === 'terminal') ?? payments.data?.[0])?.id;
  const payment = (payments.data ?? []).find((p) => p.id === (paymentId ?? defaultPaymentId));
  const otherPayment = payment?.params.kind === 'other' && !mixed;
  const baseTotals = rawLines.reduce((a, l) => ({ qty: a.qty + l.qty, sum: a.sum + l.qty * l.price }), { qty: 0, sum: 0 });
  const autoType = (priceTypes.data ?? []).find((t) => {
    const r = t.rules;
    const set = [r.minQty != null, r.minTotal != null, !!r.paymentMethodId, r.byClient === true].filter(Boolean).length;
    if (set === 0) return false;
    return (r.minQty == null || baseTotals.qty >= r.minQty) && (r.minTotal == null || baseTotals.sum >= r.minTotal)
      && (!r.paymentMethodId || r.paymentMethodId === payment?.id) && (r.byClient !== true || client?.preferredPriceTypeId === t.id);
  })?.id ?? client?.preferredPriceTypeId ?? null;
  const effectiveType = typeChoice === 'base' ? null : typeChoice || autoType;
  const typeMap = useLoad(
    () => (effectiveType && storeId ? api<Record<string, number>>(`/price-types/${effectiveType}/effective`, { query: { storeId } }) : Promise.resolve(null)),
    [effectiveType, storeId],
  );
  const lines = useMemo(() => rawLines.map((l) => (l.productId && typeMap.data && effectiveType && typeMap.data[l.productId] !== undefined ? { ...l, price: typeMap.data[l.productId] } : l)), [rawLines, typeMap.data, effectiveType]);
  const mods = (modifiers.data ?? []).filter((m) => (m.params as { stores?: Record<string, boolean> }).stores?.[storeId] !== false);
  const markup = Number(purchase?.params.markupPercent ?? 0);
  const second = mixed ? (payments.data ?? []).find((p) => p.id === (secondId ?? (payments.data ?? []).find((x) => x.id !== payment?.id)?.id)) : undefined;
  const isCash = payment?.params.kind === 'cash' && !second;
  // постоянная скидка клиента действует, пока скидка в чеке не задана вручную (как на сервере)
  const clientDiscount = ((): Discount => {
    const d = client?.discountId ? (discounts.data ?? []).find((x) => x.id === client.discountId) : undefined;
    const p = d?.params as { type?: string; value?: number } | undefined;
    if (!p || !(Number(p.value) > 0)) return { type: 'none' };
    return p.type === 'amount' ? { type: 'amount', value: Number(p.value) } : { type: 'percent', value: Number(p.value) };
  })();
  const effDiscount: Discount = discount.type === 'none' ? clientDiscount : discount;
  const totals = saleTotals(lines, markup, effDiscount);
  const bonusMax = client ? Math.max(0, Math.min(client.bonusBalance, totals.total)) : 0;
  const bonus = Math.min(Math.max(Number(bonusInput.replace(',', '.')) || 0, 0), bonusMax);
  const payable = Math.round((totals.total - bonus) * 100) / 100;
  const back = change(Number(received.replace(',', '.')) || 0, payable);
  const first = Math.min(Math.max(Number(firstValue.replace(',', '.')) || 0, 0), payable);
  const secondValue = Math.round((payable - first) * 100) / 100;
  const mixedInvalid = !!second && (first <= 0 || secondValue <= 0);

  // поиск клиента по имени или телефону
  const visible = (products.data ?? []).filter((p) => {
    const sp = p.stores.find((x) => x.storeId === storeId);
    if (p.hiddenInNewSale) return false;
    if (cat !== 'all' && p.categoryId !== cat) return false;
    const q = search.trim().toLowerCase();
    // число в поиске — ищем и по цене: «1500», «1 500», «1500₽»; пока печатают («150») — цены, начинающиеся с этих цифр
    const digits = q.replace(/[\s₽р.,]/g, '');
    const byPrice = /^\d+$/.test(digits) && String(Math.round(sp?.salePrice ?? -1)).startsWith(digits);
    return !q || byPrice || p.name.toLowerCase().includes(q) || p.barcodes.some((b) => b.includes(q));
  });

  function add(p: Product) {
    const price = p.stores.find((x) => x.storeId === storeId)?.salePrice ?? 0;
    setLines((ls) => {
      const i = flag('mergeSimilarItems', true) ? ls.findIndex((l) => l.productId === p.id) : -1;
      if (i >= 0) return ls.map((l, k) => (k === i ? { ...l, qty: l.qty + 1 } : l));
      return [...ls, { key: flag('mergeSimilarItems', true) ? p.id : `${p.id}-${Date.now()}-${ls.length}`, productId: p.id, name: p.name, price, qty: 1 }];
    });
    if (!wide) setCatalogOpen(false);
  }

  function setQty(key: string, delta: number) {
    setLines((ls) => ls.flatMap((l) => (l.key === key ? (l.qty + delta > 0 ? [{ ...l, qty: l.qty + delta }] : []) : [l])));
  }

  function reset() {
    setLines([]);
    setBonusInput('');
    setSaleComment('');
    setSaleDate('');
    setTableNo('');
    setCashbackOn(true);
    setDeliveryOn(false);
    setDeliveryAddress('');
    setDeliveryPhone('');
    setClient(null);
    setClientPending(false);
    setClientDraft({ phone: '', name: '' });
    setDiscount({ type: 'none' });
    setReceived('');
    setError(null);
    idem.current = crypto.randomUUID();
  }

  // товары, отмеченные в филиале как отсутствующие на складе: продать можно, но только после подтверждения
  const unavailable = lines.filter((l) => l.productId && products.data?.find((p) => p.id === l.productId)?.stores.some((x) => x.storeId === storeId && !x.active));
  function checkout() {
    if (unavailable.length > 0) setModal('unavailable');
    else void doCheckout(false);
  }

  async function doCheckout(confirmed: boolean) {
    if (clientPending && !clientDraft.phone) { setError('Укажите полный телефон клиента'); return; }
    setBusy(true);
    setError(null);
    try {
      let saleClient = client;
      if (!saleClient && clientDraft.phone) {
        const find = async () => (await api<Client[]>('/clients', { query: { search: clientDraft.phone } }))
          .find((c) => normalizedPhone(c.phone) === clientDraft.phone);
        saleClient = await find() ?? null;
        if (!saleClient) {
          try {
            saleClient = await api<Client>('/clients', { body: { name: clientDraft.name || `Клиент ${clientDraft.phone.slice(-4)}`, phone: clientDraft.phone } });
          } catch (e) {
            if (!(e instanceof ApiError) || e.status !== 409) throw e;
            saleClient = await find() ?? null;
            if (!saleClient) throw e;
          }
        }
        setClient(saleClient);
      }
      const sale = await api<Sale>('/sales', {
        body: {
          storeId,
          allowUnavailable: confirmed || undefined,
          clientId: saleClient?.id,
          paymentMethodId: payment?.id,
          priceTypeId: effectiveType ?? undefined,
          secondPaymentMethodId: second?.id,
          firstPaymentValue: second ? first : undefined,
          secondPaymentValue: second ? secondValue : undefined,
          purchaseMethodId: purchase?.id,
          delivery: deliveryOn || purchase?.params.delivery === true,
          deliveryAddress: deliveryOn ? deliveryAddress || undefined : undefined,
          deliveryPhone: deliveryOn ? deliveryPhone || undefined : undefined,
          bonusReduction: bonus > 0 ? bonus : undefined,
          cashbackEnabled: client ? cashbackOn : undefined,
          discountPercentage: discount.type === 'percent' ? discount.value : undefined,
          discountAmount: discount.type === 'amount' ? discount.value : undefined,
          idempotencyKey: idem.current,
          comment: [tableNo ? `Стол ${tableNo}` : '', saleComment].filter(Boolean).join('. ') || undefined,
          saleDate: saleDate ? new Date(`${saleDate}T12:00:00`).toISOString() : undefined,
          items: lines.map((l) => (l.productId ? { productId: l.productId, qty: l.qty, price: l.price } : { name: l.name, qty: l.qty, price: l.price })),
        },
      });
      if (isPainting) {
        const names = lines.map((l) => l.name);
        const store = s.stores.find((x) => x.id === storeId);
        setPainting({ ...emptyPainting(staffLabel(s.user?.name ?? '', store?.name ?? '')), ...guessPainting(names),
          payment: rub(payable), comment: names.join(', ').slice(0, 300), paymentStatus: 'полная оплата' });
        setIsPainting(false);
      }
      setDone(sale);
      reset();
      stock.reload();
    } catch (e) {
      const message = e instanceof Error ? e.message : 'Ошибка';
      if (message.includes('сначала откройте смену')) {
        pendingCheckout.current = confirmed;
        setOpeningError('');
        setModal('opening-shift');
      } else setError(message);
    } finally {
      setBusy(false);
    }
  }

  async function postpone() {
    try {
      await api('/saved-carts', { body: { storeId, name: client?.name ?? '', payload: { lines: rawLines, clientId: client?.id ?? null, discount, paymentId, purchaseId, deliveryOn, deliveryAddress, deliveryPhone } } });
      reset();
      carts.reload();
      say('Чек отложен');
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Ошибка');
    }
  }

  async function restore(c: SavedCart) {
    const pl = c.payload as { lines?: CartLine[]; clientId?: string | null; discount?: Discount; paymentId?: string | null; purchaseId?: string | null; deliveryOn?: boolean; deliveryAddress?: string; deliveryPhone?: string };
    setLines((pl.lines ?? []) as CartLine[]);
    if (pl.discount) setDiscount(pl.discount);
    if (pl.paymentId) setPaymentId(pl.paymentId);
    if (pl.purchaseId) setPurchaseId(pl.purchaseId);
    setDeliveryOn(pl.deliveryOn === true);
    setDeliveryAddress(pl.deliveryAddress ?? '');
    setDeliveryPhone(pl.deliveryPhone ?? '');
    if (pl.clientId) setClient(await api<Client>(`/clients/${pl.clientId}`).catch(() => null));
    await api(`/saved-carts/${c.id}`, { method: 'DELETE' });
    carts.reload();
    setModal(null);
  }

  const catalog = (
    <section aria-label="Каталог">
      <input type="search" placeholder="Поиск по названию или цене" value={search} onChange={(e) => setSearch(e.target.value)} style={{ marginBottom: 12 }} />
      <div className="chips">
        <button className={`chip small${cat === 'all' ? ' active' : ''}`} onClick={() => setCat('all')}>Все</button>
        {(cats.data ?? []).map((c) => (
          <button key={c.id} className={`chip small${cat === c.id ? ' active' : ''}`} onClick={() => setCat(c.id)}>{c.name}</button>
        ))}
      </div>
      {products.loading ? <Spinner /> : (
        <div className="products">
          {s.can('sale.custom_product') && (
            <button className="product" onClick={() => setModal('custom')}>
              <span className="name">Кастомный продукт</span>
              <span className="meta">свободная цена</span>
            </button>
          )}
          {visible.map((p) => {
            const price = p.stores.find((x) => x.storeId === storeId)?.salePrice ?? 0;
            const st = stockMap.get(p.id);
            const off = p.stores.some((x) => x.storeId === storeId && !x.active);
            return (
              <button key={p.id} className="product" style={off ? { opacity: 0.55 } : undefined} onClick={() => add(p)}>
                <span className="name">{p.name}{off && <div className="meta low">нет на складе</div>}</span>
                <span>
                  <span className="price">{rub(price)}</span>
                  {st && !p.isService && <div className={`meta${st.low ? ' low' : ''}`}>({num(st.qty)} шт)</div>}
                </span>
              </button>
            );
          })}
        </div>
      )}
      {!products.loading && visible.length === 0 && <div className="empty">Ничего не найдено</div>}
    </section>
  );

  const cart = (
    <>
      <div className="row" style={{ marginBottom: 14 }}>
        {s.moduleOn('simple_sales') && s.can('sale.new') && (
          <button className="chip small" onClick={() => setModal('carts')}>Открытые чеки ({carts.data?.length ?? 0})</button>
        )}
        {mods.length > 0 && <button className="chip small" aria-label="Добавки" onClick={() => setModal('mods')}>+ Добавки</button>}
        {s.can('sale.apply_discount') && (
          <button className={`chip small${discount.type !== 'none' ? ' active' : ''}`} aria-label="Скидка" onClick={() => setModal('discount')}>%</button>
        )}
      </div>

      {s.can('client.card') && <ClientPicker client={client} onChange={setClient} onPending={setClientPending} onDraft={setClientDraft} />}

      <div className="chips">
        {(purchases.data ?? []).map((p) => (
          <button key={p.id} className={`chip small${p.id === purchase?.id ? ' active' : ''}`} aria-pressed={p.id === purchase?.id} onClick={() => setPurchaseId(p.id)}><MethodPixelIcon method={p} purchase />{String(p.params.label ?? '') || p.name}</button>
        ))}
      </div>
      <div className="chips">
        {(payments.data ?? []).map((p) => (
          <button key={p.id} className={`chip small${p.id === payment?.id ? ' active' : ''}`} aria-pressed={p.id === payment?.id} onClick={() => setPaymentId(p.id)}><MethodPixelIcon method={p} />{String(p.params.label ?? '') || p.name}</button>
        ))}
      </div>
      {otherPayment && (
        <label className="field"><span>Чем оплачено — заметка</span>
          <input type="text" value={saleComment} placeholder="например: оплата по ссылке, бартер" onChange={(e) => setSaleComment(e.target.value)} />
        </label>
      )}
      {(priceTypes.data ?? []).length > 0 && (
        <label className="field"><span>Тип цены</span>
          <select value={typeChoice} onChange={(e) => setTypeChoice(e.target.value)}>
            <option value="">Авто{effectiveType ? `: ${(priceTypes.data ?? []).find((t) => t.id === effectiveType)?.name ?? ''}` : ': базовая'}</option>
            <option value="base">Базовая цена</option>
            {(priceTypes.data ?? []).map((t) => <option key={t.id} value={t.id}>{t.name}</option>)}
          </select>
        </label>
      )}
      {(payments.data ?? []).length > 1 && (
        <label className="row" style={{ gap: 8, marginBottom: 8 }}>
          <input type="checkbox" checked={mixed} onChange={(e) => setMixed(e.target.checked)} />
          <span>Смешанная оплата</span>
        </label>
      )}
      {second && (
        <>
          <div className="chips">
            {(payments.data ?? []).filter((p) => p.id !== payment?.id).map((p) => (
              <button key={p.id} className={`chip small${p.id === second.id ? ' active' : ''}`} aria-pressed={p.id === second.id} onClick={() => setSecondId(p.id)}><MethodPixelIcon method={p} />{p.name}</button>
            ))}
          </div>
          <label className="field"><span>Сумма {payment?.name}</span>
            <input type="number" min="0" step="any" value={firstValue} placeholder="0" onChange={(e) => setFirstValue(e.target.value)} />
          </label>
          <div className="total-row"><span>{second.name}</span><span>{rub(secondValue)}</span></div>
        </>
      )}

      {lines.map((l) => (
        <div className="cart-line" key={l.key}>
          <span className="grow">{l.name}</span>
          <button className="qty-btn" aria-label="Меньше" onClick={() => setQty(l.key, -1)}><Icon name="minus" size={16} /></button>
          <span style={{ minWidth: 22, textAlign: 'center' }}>{l.qty}</span>
          <button className="qty-btn" aria-label="Больше" onClick={() => setQty(l.key, 1)}><Icon name="plus" size={16} /></button>
          <span style={{ minWidth: 70, textAlign: 'right' }}>{rub(l.price * l.qty)}</span>
        </div>
      ))}

      {totals.discount > 0 && <div className="row muted" style={{ justifyContent: 'space-between' }}><span>Скидка</span><span>−{rub(totals.discount)}</span></div>}
      {totals.markup > 0 && <div className="row muted" style={{ justifyContent: 'space-between' }}><span>Надбавка</span><span>+{rub(totals.markup)}</span></div>}
      {bonus > 0 && <div className="row muted" style={{ justifyContent: 'space-between' }}><span>Оплата бонусами</span><span>−{rub(bonus)}</span></div>}
      <div className="total-row"><span>Итого</span><span>{rub(payable)}</span></div>
      {client && (
        <div style={{ marginBottom: 12 }}>
          {bonusMax > 0 && (
            <label className="field"><span>Списать бонусы (доступно {rub(client.bonusBalance)})</span>
              <input type="number" min="0" max={bonusMax} step="any" value={bonusInput} placeholder="0" onChange={(e) => setBonusInput(e.target.value)} />
            </label>
          )}
          <label className="row" style={{ gap: 8 }}>
            <input type="checkbox" checked={cashbackOn} onChange={(e) => setCashbackOn(e.target.checked)} />
            <span>Начислять кэшбэк</span>
          </label>
        </div>
      )}
      {flag('dateFieldVisible', false) && <label className="field"><span>Дата продажи</span><input type="date" value={saleDate} max={new Date().toISOString().slice(0, 10)} onChange={(e) => setSaleDate(e.target.value)} /></label>}
      {flag('tableNumberFieldVisible', false) && <label className="field"><span>Номер стола</span><input type="text" value={tableNo} onChange={(e) => setTableNo(e.target.value)} /></label>}
      {flag('commentFieldVisible', false) && !otherPayment && <label className="field"><span>Комментарий</span><input type="text" value={saleComment} onChange={(e) => setSaleComment(e.target.value)} /></label>}
      <label className="row" style={{ gap: 8, marginBottom: 8 }}>
        <input type="checkbox" checked={deliveryOn} onChange={(e) => { setDeliveryOn(e.target.checked); if (e.target.checked) { if (!deliveryAddress && client?.address) setDeliveryAddress(client.address); if (!deliveryPhone && client?.phone) setDeliveryPhone(client.phone); } }} />
        <span>Доставка</span>
      </label>
      {deliveryOn && (
        <>
          <label className="field"><span>Адрес доставки</span><input type="text" value={deliveryAddress} onChange={(e) => setDeliveryAddress(e.target.value)} /></label>
          <label className="field"><span>Телефон для доставки</span><input type="text" value={deliveryPhone} onChange={(e) => setDeliveryPhone(e.target.value)} /></label>
        </>
      )}

      {isCash && lines.length > 0 && (
        <>
          <span className="label">Сдача: {back === null ? '—' : rub(back)}</span>
          <input type="text" inputMode="decimal" placeholder="Другая сумма" value={received} onChange={(e) => setReceived(e.target.value)} />
          <div className="bills">
            {BILLS.map((b) => <button key={b} onClick={() => setReceived(String((Number(received) || 0) + b))}>{b}</button>)}
            <button aria-label="Сбросить" style={{ borderColor: 'var(--danger)', color: 'var(--danger)' }} onClick={() => setReceived('')}>X</button>
          </div>
        </>
      )}

      <label className="row" style={{ gap: 8, marginBottom: 10 }}>
        <input type="checkbox" checked={isPainting} onChange={(e) => setIsPainting(e.target.checked)} />
        <span>Это картина — записать в таблицу картин</span>
      </label>
    </>
  );

  return (
    <>
      <Header title="Новая продажа" right={lines.length > 0 ? <button className="icon-btn" aria-label="Очистить чек" onClick={reset}><Icon name="trash" /></button> : undefined} />
      <main className="page wide sale-page">
        <div className="sale">
          <section className="sale-cart" aria-label="Чек">
            <div className="sale-cart-scroll">
              {!wide && <button className="btn light" style={{ marginBottom: 16 }} onClick={() => setCatalogOpen(true)}>+ Добавить товар</button>}
              {cart}
            </div>
            <div className="sale-cart-footer">
              {error && <div className="error" role="alert">{error}</div>}
              {lines.length === 0 && <div id="sale-checkout-hint" className="muted sale-checkout-hint">Добавьте товар, чтобы оформить продажу</div>}
              <div className="sale-cart-actions">
                {flag('showSaveCartForLater', true) && <button className="btn light inline sale-postpone" disabled={lines.length === 0} onClick={postpone}>Отложить</button>}
                <button className="btn sale-checkout" data-payment={second ? 'mixed' : paymentAppearance(payment)}
                  aria-describedby={lines.length === 0 ? 'sale-checkout-hint' : undefined}
                  disabled={lines.length === 0 || busy || mixedInvalid || clientPending} onClick={checkout}>Готово</button>
              </div>
            </div>
          </section>
          {wide && <div className="sale-catalog-scroll">{catalog}</div>}
        </div>
        {!wide && catalogOpen && (
          <div className="sheet">
            <Header title="Каталог" back={false} right={<button className="icon-btn" aria-label="Закрыть каталог" onClick={() => setCatalogOpen(false)}><Icon name="close" /></button>} />
            <div className="page">{catalog}</div>
          </div>
        )}
      </main>

      {modal === 'mods' && (
        <Modal onClose={() => setModal(null)}>
          <div className="h2" style={{ marginTop: 0 }}>Добавки</div>
          {mods.map((m) => (
            <div key={m.id}>
              <div className="label">{m.name}</div>
              {((m.params as { items?: { productId: string; name: string; price: number }[] }).items ?? []).map((i) => (
                <div key={i.productId} className="card click row" onClick={() => {
                  setLines((ls) => {
                    const k = `mod-${i.productId}`;
                    const at = ls.findIndex((l) => l.key === k);
                    if (at >= 0) return ls.map((l, j) => (j === at ? { ...l, qty: l.qty + 1 } : l));
                    return [...ls, { key: k, productId: i.productId, name: i.name, price: i.price, qty: 1 }];
                  });
                  setModal(null);
                }}>
                  <div className="grow title">{i.name}</div><span className="price">+{rub(i.price)}</span>
                </div>
              ))}
            </div>
          ))}
        </Modal>
      )}
      {modal === 'unavailable' && (
        <Modal onClose={() => setModal(null)}>
          <div className="h2" style={{ marginTop: 0 }}>Этот товар отмечен как отсутствующий на складе</div>
          <div className="muted" style={{ marginBottom: 12 }}>{unavailable.map((l) => l.name).join(', ')}</div>
          <div style={{ display: 'grid', gap: 10 }}>
            <button className="btn" onClick={() => { setModal(null); void doCheckout(true); }}>Я знаю, что делаю</button>
            <button className="btn light" onClick={() => setModal(null)}>Передумал</button>
          </div>
        </Modal>
      )}
      {modal === 'opening-shift' && (
        <Modal onClose={() => setModal(null)}>
          <div className="h2" style={{ marginTop: 0 }}>Открыть смену перед продажей</div>
          <p className="muted">Укажите, сколько наличных в кассе на начало дня. Чек сохранится после открытия смены.</p>
          <label className="field"><span>Наличные на начало смены, ₽</span><input type="number" min="0.01" step="any" inputMode="decimal" value={openingCash} onChange={(e) => setOpeningCash(e.target.value)} autoFocus /></label>
          {openingError && <div className="error" role="alert">{openingError}</div>}
          <button className="btn" disabled={busy || !(Number(openingCash.replace(',', '.')) > 0)} onClick={async () => {
            setBusy(true); setOpeningError('');
            try {
              await api('/shifts/open', { body: { storeId, openingCash: Number(openingCash.replace(',', '.')) } });
              setModal(null); setOpeningCash('');
              void doCheckout(pendingCheckout.current);
            } catch (e) { setOpeningError(e instanceof Error ? e.message : 'Не удалось открыть смену'); }
            finally { setBusy(false); }
          }}>Открыть смену и сохранить чек</button>
        </Modal>
      )}
      {modal === 'discount' && (
        <Modal onClose={() => setModal(null)}>
          <div className="h2" style={{ marginTop: 0 }}>Скидка</div>
          <div className="chips">
            <button className={`chip${discount.type === 'none' ? ' active' : ''}`} onClick={() => { setDiscount({ type: 'none' }); setModal(null); }}>Без скидки</button>
            {(discounts.data ?? []).map((d) => {
              const v = Number(d.params.value);
              const pick: Discount = d.params.type === 'amount' ? { type: 'amount', value: v } : { type: 'percent', value: v };
              return <button key={d.id} className="chip" onClick={() => { setDiscount(pick); setModal(null); }}>{d.name}</button>;
            })}
          </div>
        </Modal>
      )}

      {modal === 'custom' && (
        <Modal onClose={() => setModal(null)}>
          <div className="h2" style={{ marginTop: 0 }}>Кастомный продукт</div>
          <label className="field"><span>Название</span><input type="text" value={custom.name} onChange={(e) => setCustom({ ...custom, name: e.target.value })} /></label>
          <label className="field"><span>Цена</span><input type="number" min="0" value={custom.price} onChange={(e) => setCustom({ ...custom, price: e.target.value })} /></label>
          <button
            className="btn"
            disabled={!custom.name.trim() || custom.price === '' || Number(custom.price) < 0}
            onClick={() => {
              setLines((ls) => [...ls, { key: `c-${Date.now()}`, name: custom.name.trim(), price: Number(custom.price), qty: 1 }]);
              setCustom({ name: '', price: '' });
              setModal(null);
              if (!wide) setCatalogOpen(false);
            }}
          >Добавить</button>
        </Modal>
      )}

      {modal === 'carts' && (
        <Modal onClose={() => setModal(null)}>
          <div className="h2" style={{ marginTop: 0 }}>Открытые чеки</div>
          {(carts.data ?? []).length === 0 && <div className="empty">Открытых чеков нет</div>}
          {(carts.data ?? []).map((c) => (
            <div key={c.id} className="card click" onClick={() => restore(c)}>
              <div className="title">{c.name || 'Без клиента'}</div>
              <div className="sub">{(c.payload.lines ?? []).length} поз.</div>
            </div>
          ))}
        </Modal>
      )}

      {painting && (
        <Modal onClose={() => setPainting(null)}>
          <PaintingForm initial={painting} onCancel={() => setPainting(null)}
            onSaved={(r) => { setPainting(null); say(r.tracker ? `Картина в таблице, трекер ${r.tracker}` : 'Картина записана в таблицу'); }} />
        </Modal>
      )}
      {done && !painting && (
        <Modal onClose={() => setDone(null)}>
          <div className="h2" style={{ marginTop: 0 }}>Успешно</div>
          <p>Продажа на {rub(done.total)} оформлена.</p>
          <div className="row">
            <button className="btn light" onClick={() => setDone(null)}>Новая продажа</button>
            <button className="btn" onClick={() => nav(`/sale/${done.id}`)}>Открыть чек</button>
          </div>
        </Modal>
      )}
      {toast}
    </>
  );
}
