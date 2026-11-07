import { useState } from 'react';
import { api } from '../api';
import { fmtDateTime } from '../dates';
import { Header, Spinner, useLoad, useToast } from '../ui';

interface Draft {
  id: string;
  kind: 'sale' | 'acceptance';
  storeId: string;
  payload: { items?: { productId: string; qty: number; price?: number | null }[]; comment?: string | null; supplierId?: string | null };
  status: 'proposed' | 'expired' | 'applied' | 'dismissed';
  expiresAt: string;
  tokenName: string;
  createdAt: string;
}
const STATUS: Record<string, string> = { proposed: 'Предложен', expired: 'Истёк', applied: 'Применён', dismissed: 'Отклонён' };
const FILTERS: [string, string][] = [['proposed', 'Предложенные'], ['applied', 'Применённые'], ['dismissed', 'Отклонённые'], ['', 'Все']];

/** Черновики от ИИ-агента: человек проверяет и применяет (создаётся реальная продажа или приёмка) либо отклоняет. */
export default function AgentDrafts() {
  const [toast, say] = useToast();
  const [status, setStatus] = useState('proposed');
  const list = useLoad(() => api<Draft[]>('/api-drafts', { query: { status: status || undefined } }), [status]);

  async function apply(d: Draft) {
    const items = (d.payload.items ?? []).map((i) => ({ productId: i.productId, qty: i.qty, ...(i.price != null ? { price: i.price } : {}) }));
    try {
      if (d.kind === 'sale') {
        const sale = await api<{ id: string }>('/sales', { body: { storeId: d.storeId, items, comment: d.payload.comment ?? undefined } });
        await api(`/api-drafts/${d.id}/applied`, { body: { saleId: sale.id } });
      } else {
        const sup = await api<{ id: string }>('/supplies', { body: { storeId: d.storeId, supplierId: d.payload.supplierId ?? undefined, comment: d.payload.comment ?? undefined, items } });
        await api(`/api-drafts/${d.id}/applied`, { body: { supplyId: sup.id } });
      }
      say('Применено');
      list.reload();
    } catch (e) {
      say(e instanceof Error ? e.message : 'Ошибка');
    }
  }

  async function dismiss(d: Draft) {
    try {
      await api(`/api-drafts/${d.id}/dismiss`, { method: 'POST' });
      list.reload();
    } catch (e) {
      say(e instanceof Error ? e.message : 'Ошибка');
    }
  }

  return (
    <>
      <Header title="Черновики от ИИ-агента" />
      <main className="page">
        <div className="chips">
          {FILTERS.map(([k, l]) => <button key={k} className={`chip${status === k ? ' active' : ''}`} onClick={() => setStatus(k)}>{l}</button>)}
        </div>
        {list.loading && <Spinner />}
        {list.error && <div className="error">{list.error}</div>}
        {!list.loading && (list.data ?? []).length === 0 && <div className="empty">Черновиков нет</div>}
        {(list.data ?? []).map((d) => (
          <div key={d.id} className="card">
            <div className="title">{d.kind === 'sale' ? 'Продажа' : 'Приёмка'} · {STATUS[d.status]}</div>
            <div className="sub">{fmtDateTime(d.createdAt)} · {d.tokenName} · позиций {d.payload.items?.length ?? 0}{d.payload.comment ? ` · ${d.payload.comment}` : ''}</div>
            {d.status === 'proposed' && (
              <div className="row" style={{ marginTop: 8 }}>
                <button className="btn light" onClick={() => dismiss(d)}>Отклонить</button>
                <button className="btn" onClick={() => apply(d)}>Применить</button>
              </div>
            )}
          </div>
        ))}
      </main>
      {toast}
    </>
  );
}
