import { useState } from 'react';
import { api } from '../api';
import { fmtDateTime } from '../dates';
import { Header, Modal, Spinner, useLoad, useToast } from '../ui';

interface Token { id: string; name: string; prefix: string; scopes: string[]; status: string; expiresAt: string; lastUsedAt: string | null; createdBy: string }
interface Audit { id: string; tokenName: string | null; action: string; allowed: boolean; text: string; createdAt: string }

const SCOPES: [string, string][] = [
  ['read', 'Чтение всего'],
  ['read:products', 'Чтение товаров'],
  ['read:stock', 'Чтение остатков'],
  ['read:sales', 'Чтение продаж и отчётов'],
  ['draft:sales', 'Черновики продаж'],
  ['draft:acceptances', 'Черновики приёмок'],
];
const STATUS: Record<string, string> = { active: 'Активен', revoked: 'Отозван', expired: 'Истёк' };

/** API-токены для внешних систем и ИИ-агентов: создание (секрет виден один раз), отзыв, журнал действий. */
export default function ApiTokens() {
  const [toast, say] = useToast();
  const [tab, setTab] = useState<'tokens' | 'audit'>('tokens');
  const tokensRes = useLoad(() => api<{ canUseApi: boolean; tokens: Token[] }>('/api-tokens'), []);
  const tokens = { ...tokensRes, data: tokensRes.data?.tokens ?? null };
  const audit = useLoad(() => (tab === 'audit' ? api<Audit[]>('/api-tokens/audit') : Promise.resolve([] as Audit[])), [tab]);
  const [form, setForm] = useState(false);
  const [name, setName] = useState('');
  const [scopes, setScopes] = useState<string[]>(['read']);
  const [days, setDays] = useState('30');
  const [secret, setSecret] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  async function create() {
    setError(null);
    try {
      const r = await api<{ secret: string }>('/api-tokens', { body: { name, scopes, expiresInDays: Number(days) } });
      setSecret(r.secret);
      setForm(false);
      setName('');
      tokens.reload();
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Ошибка');
    }
  }

  async function revoke(id: string) {
    if (!confirm('Отозвать токен? Внешние системы потеряют доступ.')) return;
    try {
      await api(`/api-tokens/${id}/revoke`, { method: 'POST' });
      tokens.reload();
    } catch (e) {
      say(e instanceof Error ? e.message : 'Ошибка');
    }
  }

  return (
    <>
      <Header title="API-токены" />
      <main className="page">
        <div className="chips">
          <button className={`chip${tab === 'tokens' ? ' active' : ''}`} onClick={() => setTab('tokens')}>Токены</button>
          <button className={`chip${tab === 'audit' ? ' active' : ''}`} onClick={() => setTab('audit')}>Журнал</button>
        </div>
        {tab === 'tokens' && tokensRes.data?.canUseApi === false && <div className="error">API недоступен для вашей организации</div>}
        {tab === 'tokens' && (
          <>
            <button className="btn light" style={{ marginBottom: 16 }} onClick={() => setForm(true)}>+ Создать токен</button>
            {tokens.loading && <Spinner />}
            {(tokens.data ?? []).map((t) => (
              <div key={t.id} className="card">
                <div className="row">
                  <div className="grow">
                    <div className="title">{t.name} <span className="muted">{t.prefix}…</span></div>
                    <div className="sub">{STATUS[t.status] ?? t.status} · до {fmtDateTime(t.expiresAt)} · {t.lastUsedAt ? `использован ${fmtDateTime(t.lastUsedAt)}` : 'не использовался'}</div>
                    <div className="sub">{t.scopes.join(', ')} · создал {t.createdBy}</div>
                  </div>
                  {t.status === 'active' && <button className="link" onClick={() => revoke(t.id)}>Отозвать</button>}
                </div>
              </div>
            ))}
          </>
        )}
        {tab === 'audit' && (
          <>
            {audit.loading && <Spinner />}
            {(audit.data ?? []).length === 0 && !audit.loading && <div className="empty">Записей нет</div>}
            {(audit.data ?? []).map((a) => (
              <div key={a.id} className="card">
                <div className="title">{a.allowed ? '' : '⛔ '}{a.text}</div>
                <div className="sub">{fmtDateTime(a.createdAt)} · {a.tokenName ?? '—'} · {a.action}</div>
              </div>
            ))}
          </>
        )}
      </main>
      {form && (
        <Modal onClose={() => setForm(false)}>
          <div className="h2" style={{ marginTop: 0 }}>Новый токен</div>
          <label className="field"><span>Название</span><input type="text" value={name} placeholder="ИИ-агент" onChange={(e) => setName(e.target.value)} /></label>
          {SCOPES.map(([k, l]) => (
            <label key={k} className="row" style={{ gap: 8, marginBottom: 8 }}>
              <input type="checkbox" checked={scopes.includes(k)} style={{ width: 20, height: 20 }} onChange={(e) => setScopes(e.target.checked ? [...scopes, k] : scopes.filter((x) => x !== k))} />
              <span>{l}</span>
            </label>
          ))}
          <label className="field"><span>Срок действия, дней</span><input type="number" min="1" max="365" value={days} onChange={(e) => setDays(e.target.value)} /></label>
          {error && <div className="error" role="alert">{error}</div>}
          <button className="btn" disabled={!name.trim() || scopes.length === 0 || !(Number(days) > 0)} onClick={create}>Создать</button>
        </Modal>
      )}
      {secret && (
        <Modal onClose={() => setSecret(null)}>
          <div className="h2" style={{ marginTop: 0 }}>Токен создан</div>
          <div className="sub">Скопируйте секрет сейчас: больше он показан не будет.</div>
          <input type="text" readOnly value={secret} onFocus={(e) => e.target.select()} style={{ margin: '12px 0' }} />
          <button className="btn" onClick={() => { void navigator.clipboard?.writeText(secret).catch(() => undefined); setSecret(null); }}>Скопировать и закрыть</button>
        </Modal>
      )}
      {toast}
    </>
  );
}
