import { useEffect, useState } from 'react';
import { api } from '../api';
import { fmtDateTime } from '../dates';
import { useSession } from '../session';
import { Header, Spinner, StoreChips, useLoad } from '../ui';

interface Ev { id: string; storeId: string | null; author: string | null; kind: string; text: string; createdAt: string; unread: boolean }

/** Лента обновлений: что менялось в данных (кто и когда); при открытии всё помечается прочитанным. */
export default function Updates() {
  const s = useSession();
  const [store, setStore] = useState('all');
  const list = useLoad(() => api<Ev[]>('/updates', { query: { storeId: store === 'all' ? undefined : store } }), [store]);
  useEffect(() => {
    if (list.data && list.data.some((e) => e.unread)) void api('/updates/read', { method: 'POST' }).catch(() => undefined);
  }, [list.data]);

  return (
    <>
      <Header title="Обновления" />
      <main className="page">
        <StoreChips all stores={s.stores} value={store} onChange={setStore} />
        {list.loading && <Spinner />}
        {list.error && <div className="error">{list.error}</div>}
        {!list.loading && (list.data ?? []).length === 0 && <div className="empty">Обновлений пока нет</div>}
        {(list.data ?? []).map((e) => (
          <div key={e.id} className="card">
            <div className="title">{e.unread ? '● ' : ''}{e.text}</div>
            <div className="sub">{fmtDateTime(e.createdAt)}{e.author ? ` · ${e.author}` : ''} · {e.kind}</div>
          </div>
        ))}
      </main>
    </>
  );
}
