import { useEffect, useMemo, useRef, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { api, type PriceType, type Category, type Client, type ListItem, type Product, type Sale, type SavedCart, type StockRow } from '../api';
import { Icon } from '../Icon';
import { change, num, rub, saleTotals, type CartLine, type Discount } from '../money';
import { useSession } from '../session';
import { Header, Modal, Spinner, useLoad, useMedia, useToast } from '../ui';

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

  const modifiers = useLoad(() => api<ListItem[]>('/lists/product_modifiers'), []);
  const priceTypes = useLoad(() => api<PriceType[]>('/price-types'), []);
  const [rawLines, setLines] = useState<CartLine[]>([]);
  const [typeChoice, setTypeChoice] = useState<string>(''); // '' — авто, 'base' — базовая, иначе id типа цены
  const [search, setSearch] = useState('');
  const [cat, setCat] = useState<string>('all');
  const [catalogOpen, setCatalogOpen] = useState(false);
  const [client, setClient] = useState<Client | null>(null);
  const [clientQuery, setClientQuery] = useState('');
  const [found, setFound] = useState<Client[]>([]);
  const [discount, setDiscount] = useState<Discount>({ type: 'none' });
  const [purchaseId, setPurchaseId] = useState<string | null>(null);
  const [paymentId, setPaymentId] = useState<string | null>(null);
  const [mixed, setMixed] = useState(false);
  const [secondId, setSecondId] = useState<string | null>(null);
  const [firstValue, setFirstValue] = useState('');
  const [received, setReceived] = useState('');
  const [modal, setModal] = useState<null | 'discount' | 'custom' | 'carts' | 'mods'>(null);
  const [custom, setCustom] = useState({ name: '', price: '' });
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [done, setDone] = useState<Sale | null>(null);
  const idem = useRef(crypto.randomUUID());

  const stockMap = useMemo(() => new Map((stock.data ?? []).map((r) => [r.productId, r])), [stock.data]);
  const purchase = (purchases.data ?? []).find((p) => p.id === (purchaseId ?? purchases.data?.[0]?.id));
  const payment = (payments.data ?? []).find((p) => p.id === (paymentId ?? payments.data?.[0]?.id));
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
  const totals = saleTotals(lines, markup, discount);
  const back = change(Number(received.replace(',', '.')) || 0, totals.total);
  const first = Math.min(Math.max(Number(firstValue.replace(',', '.')) || 0, 0), totals.total);
  const secondValue = Math.round((totals.total - first) * 100) / 100;
  const mixedInvalid = !!second && (first <= 0 || secondValue <= 0);

  // поиск клиента по имени или телефону
  useEffect(() => {
    if (!s.can('client.view') || clientQuery.trim().length < 2) {
      setFound([]);
      return;
    }
    const t = setTimeout(() => {
      api<Client[]>('/clients', { query: { search: clientQuery.trim() } }).then(setFound).catch(() => setFound([]));
    }, 250);
    return () => clearTimeout(t);
  }, [clientQuery, s]);

  const visible = (products.data ?? []).filter((p) => {
    const sp = p.stores.find((x) => x.storeId === storeId);
    if (p.hiddenInNewSale || (sp && !sp.active)) return false;
    if (cat !== 'all' && p.categoryId !== cat) return false;
    const q = search.trim().toLowerCase();
    return !q || p.name.toLowerCase().includes(q) || p.barcodes.some((b) => b.includes(q));
  });

  function add(p: Product) {
    const price = p.stores.find((x) => x.storeId === storeId)?.salePrice ?? 0;
    setLines((ls) => {
      const i = ls.findIndex((l) => l.productId === p.id);
      if (i >= 0) return ls.map((l, k) => (k === i ? { ...l, qty: l.qty + 1 } : l));
      return [...ls, { key: p.id, productId: p.id, name: p.name, price, qty: 1 }];
    });
    if (!wide) setCatalogOpen(false);
  }

  function setQty(key: string, delta: number) {
    setLines((ls) => ls.flatMap((l) => (l.key === key ? (l.qty + delta > 0 ? [{ ...l, qty: l.qty + delta }] : []) : [l])));
  }

  function reset() {
    setLines([]);
    setClient(null);
    setClientQuery('');
    setDiscount({ type: 'none' });
    setReceived('');
    setError(null);
    idem.current = crypto.randomUUID();
  }

  async function checkout() {
    setBusy(true);
    setError(null);
    try {
      const sale = await api<Sale>('/sales', {
        body: {
          storeId,
          clientId: client?.id,
          paymentMethodId: payment?.id,
          priceTypeId: effectiveType ?? undefined,
          secondPaymentMethodId: second?.id,
          firstPaymentValue: second ? first : undefined,
          secondPaymentValue: second ? secondValue : undefined,
          purchaseMethodId: purchase?.id,
          delivery: purchase?.params.delivery === true,
          discountPercentage: discount.type === 'percent' ? discount.value : undefined,
          discountAmount: discount.type === 'amount' ? discount.value : undefined,
          idempotencyKey: idem.current,
          items: lines.map((l) => (l.productId ? { productId: l.productId, qty: l.qty, price: l.price } : { name: l.name, qty: l.qty, price: l.price })),
        },
      });
      setDone(sale);
      reset();
      stock.reload();
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Ошибка');
    } finally {
      setBusy(false);
    }
  }

  async function postpone() {
    try {
      await api('/saved-carts', { body: { storeId, name: client?.name ?? '', payload: { lines, clientId: client?.id ?? null } } });
      reset();
      carts.reload();
      say('Чек отложен');
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Ошибка');
    }
  }

  async function restore(c: SavedCart) {
    setLines((c.payload.lines ?? []) as CartLine[]);
    await api(`/saved-carts/${c.id}`, { method: 'DELETE' });
    carts.reload();
    setModal(null);
  }

  const catalog = (
    <section aria-label="Каталог">
      <input type="search" placeholder="Поиск товаров" value={search} onChange={(e) => setSearch(e.target.value)} style={{ marginBottom: 12 }} />
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
            return (
              <button key={p.id} className="product" onClick={() => add(p)}>
                <span className="name">{p.name}</span>
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
    <section aria-label="Чек">
      <div className="row" style={{ marginBottom: 14 }}>
        {s.moduleOn('simple_sales') && s.can('sale.new') && (
          <button className="chip small" onClick={() => setModal('carts')}>Открытые чеки ({carts.data?.length ?? 0})</button>
        )}
        {mods.length > 0 && <button className="chip small" aria-label="Добавки" onClick={() => setModal('mods')}>+ Добавки</button>}
        {s.can('sale.apply_discount') && (
          <button className={`chip small${discount.type !== 'none' ? ' active' : ''}`} aria-label="Скидка" onClick={() => setModal('discount')}>%</button>
        )}
      </div>

      {s.can('client.view') && (
        <div style={{ marginBottom: 14 }}>
          <span className="label">Клиент</span>
          {client ? (
            <div className="card row">
              <div className="grow"><div className="title">{client.name}</div><div className="sub">{client.phone} · бонусы {rub(client.bonusBalance)}</div></div>
              <button className="icon-btn small" aria-label="Убрать клиента" onClick={() => setClient(null)}><Icon name="close" size={18} /></button>
            </div>
          ) : (
            <>
              <input type="search" placeholder="Поиск" value={clientQuery} onChange={(e) => setClientQuery(e.target.value)} />
              {found.map((c) => (
                <div key={c.id} className="card click" style={{ marginTop: 6 }} onClick={() => { setClient(c); setFound([]); setClientQuery(''); }}>
                  <div className="title">{c.name}</div><div className="sub">{c.phone}</div>
                </div>
              ))}
            </>
          )}
        </div>
      )}

      <div className="chips">
        {(purchases.data ?? []).map((p) => (
          <button key={p.id} className={`chip small${p.id === purchase?.id ? ' active' : ''}`} onClick={() => setPurchaseId(p.id)}>{p.name}</button>
        ))}
      </div>
      <div className="chips">
        {(payments.data ?? []).map((p) => (
          <button key={p.id} className={`chip small${p.id === payment?.id ? ' active' : ''}`} onClick={() => setPaymentId(p.id)}>{p.name}</button>
        ))}
      </div>
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
          <input type="checkbox" checked={mixed} onChange={(e) => setMixed(e.target.checked)} style={{ width: 20, height: 20 }} />
          <span>Смешанная оплата</span>
        </label>
      )}
      {second && (
        <>
          <div className="chips">
            {(payments.data ?? []).filter((p) => p.id !== payment?.id).map((p) => (
              <button key={p.id} className={`chip small${p.id === second.id ? ' active' : ''}`} onClick={() => setSecondId(p.id)}>{p.name}</button>
            ))}
          </div>
          <label className="field"><span>Сумма {payment?.name}</span>
            <input type="number" min="0" step="any" value={firstValue} placeholder="0" onChange={(e) => setFirstValue(e.target.value)} />
          </label>
          <div className="total-row"><span>{second.name}</span><span>{rub(secondValue)}</span></div>
        </>
      )}

      {lines.length === 0 && <div className="muted" style={{ margin: '16px 0' }}>Добавьте товар, чтобы оформить продажу</div>}
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
      <div className="total-row"><span>Итого</span><span>{rub(totals.total)}</span></div>

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

      {error && <div className="error" role="alert">{error}</div>}
      <button className="btn light" style={{ marginBottom: 10 }} disabled={lines.length === 0} onClick={postpone}>Отложить</button>
      <button className="btn" disabled={lines.length === 0 || busy || mixedInvalid} onClick={checkout}>Готово</button>
    </section>
  );

  return (
    <>
      <Header title="Новая продажа" right={lines.length > 0 ? <button className="icon-btn" aria-label="Очистить чек" onClick={reset}><Icon name="trash" /></button> : undefined} />
      <main className="page wide">
        {wide ? (
          <div className="sale">{cart}{catalog}</div>
        ) : (
          <>
            <button className="btn light" style={{ marginBottom: 16 }} onClick={() => setCatalogOpen(true)}>+ Добавить товар</button>
            {cart}
            {catalogOpen && (
              <div className="sheet">
                <Header title="Каталог" back={false} right={<button className="icon-btn" aria-label="Закрыть каталог" onClick={() => setCatalogOpen(false)}><Icon name="close" /></button>} />
                <div className="page">{catalog}</div>
              </div>
            )}
          </>
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

      {done && (
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
