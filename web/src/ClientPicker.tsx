import { useEffect, useState } from 'react';
import { api, ApiError, type Client } from './api';

export function normalizedPhone(input: string) {
  let digits = input.replace(/\D/g, '');
  if (digits.length === 10 && !input.trim().startsWith('+')) digits = '7' + digits;
  if (digits.length === 11 && digits.startsWith('8') && !input.trim().startsWith('+')) digits = '7' + digits.slice(1);
  return digits.length >= 10 && digits.length <= 15 ? '+' + digits : '';
}

export function ClientPicker({ client, onChange, onPending, onDraft }: {
  client: Client | null;
  onChange: (client: Client | null) => void;
  onPending: (pending: boolean) => void;
  onDraft?: (draft: { phone: string; name: string }) => void;
}) {
  const [query, setQuery] = useState('');
  const [found, setFound] = useState<Client[]>([]);
  const [adding, setAdding] = useState(false);
  const [name, setName] = useState('');
  const [phone, setPhone] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const draft = { phone: normalizedPhone(adding ? phone : query), name: adding ? name.trim() : '' };
  useEffect(() => { onDraft?.(draft); }, [draft.phone, draft.name, onDraft]);
  useEffect(() => {
    const value = query.trim();
    if (value.length < 2) { setFound([]); return; }
    let current = true;
    const timer = window.setTimeout(() => {
      api<Client[]>('/clients', { query: { search: normalizedPhone(value) || value } })
        .then((list) => { if (current) setFound(list.slice(0, 8)); })
        .catch(() => { if (current) setFound([]); });
    }, 220);
    return () => { current = false; window.clearTimeout(timer); };
  }, [query]);

  function pick(selected: Client) {
    onChange(selected);
    onPending(false);
    setAdding(false);
    setQuery('');
    setFound([]);
    setPhone('');
    setName('');
  }

  async function add() {
    if (busy) return;
    const number = normalizedPhone(phone);
    if (!number || !name.trim()) { setError('Укажите имя и полный телефон клиента'); return; }
    setBusy(true);
    setError('');
    try {
      const find = async () => (await api<Client[]>('/clients', { query: { search: number } }))
        .find((c) => normalizedPhone(c.phone) === number);
      const existing = await find();
      if (existing) { pick(existing); return; }
      try {
        pick(await api<Client>('/clients', { body: { name: name.trim(), phone: number } }));
      } catch (e) {
        if (!(e instanceof ApiError) || e.status !== 409) throw e;
        const concurrent = await find();
        if (!concurrent) throw e;
        pick(concurrent);
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Не удалось сохранить контакты клиента');
    } finally {
      setBusy(false);
    }
  }

  return (
    <section className="client-picker" aria-label="Контакты клиента">
      <span className="label">Клиент</span>
      {client ? (
        <div className="row"><span className="grow">{client.name} · {client.phone}</span>
          <button className="link" type="button" onClick={() => onChange(null)}>Убрать</button></div>
      ) : adding ? (
        <>
          <div className="sale-entry-fields">
            <label className="field"><span>Имя</span><input value={name} onChange={(e) => setName(e.target.value)} /></label>
            <label className="field"><span>Телефон</span><input type="tel" placeholder="+7" value={phone} onChange={(e) => setPhone(e.target.value)} /></label>
          </div>
          {error && <p className="error" role="alert">{error}</p>}
          <div className="row">
            <button className="btn light inline" type="button" onClick={() => void add()} disabled={busy}>{busy ? 'Сохранение…' : 'Добавить к продаже'}</button>
            <button className="link" type="button" disabled={busy} onClick={() => { setAdding(false); onPending(false); setError(''); }}>Отмена</button>
          </div>
        </>
      ) : (
        <>
          <div className="row">
            <input className="grow" type="search" aria-label="Найти клиента" placeholder="Имя или телефон" value={query} onChange={(e) => setQuery(e.target.value)} />
            <button className="btn light inline" type="button" onClick={() => {
              if (normalizedPhone(query)) setPhone(query);
              else setName(query.trim());
              setAdding(true);
              onPending(true);
            }}>Новый клиент</button>
          </div>
          {found.map((c) => <button key={c.id} className="btn light" type="button" onClick={() => pick(c)}>{c.name} · {c.phone}</button>)}
          {draft.phone && !found.some((c) => normalizedPhone(c.phone) === draft.phone) && <p className="sub">Если это новый номер, клиент создастся при сохранении чека. Имя можно добавить позже.</p>}
        </>
      )}
    </section>
  );
}
