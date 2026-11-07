import { useState } from 'react';
import { api, type Role, type StaffUser } from '../api';
import { Icon } from '../Icon';
import { useSession } from '../session';
import { Header, Modal, Spinner, useLoad, useToast } from '../ui';

/** Сотрудники: список активных и неактивных, создание, смена роли и филиалов, пароль, деактивация. */
export default function Users() {
  const s = useSession();
  const [toast, say] = useToast();
  const [inactive, setInactive] = useState(false);
  const users = useLoad(() => api<StaffUser[]>('/users', { query: { inactive } }), [inactive]);
  const roles = useLoad(() => api<Role[]>('/roles'), []);
  const [editing, setEditing] = useState<StaffUser | 'new' | null>(null);

  return (
    <>
      <Header title="Сотрудники" right={<button className="icon-btn" aria-label="Новый сотрудник" onClick={() => setEditing('new')}><Icon name="plus" /></button>} />
      <main className="page">
        <div className="chips">
          <button className={`chip${!inactive ? ' active' : ''}`} onClick={() => setInactive(false)}>Активные</button>
          <button className={`chip${inactive ? ' active' : ''}`} onClick={() => setInactive(true)}>Неактивные</button>
        </div>
        {users.loading && <Spinner />}
        {!users.loading && (users.data ?? []).length === 0 && <div className="empty">Список пуст</div>}
        {(users.data ?? []).map((u) => (
          <div key={u.id} className="card click" onClick={() => setEditing(u)}>
            <div className="title">{u.name}</div>
            <div className="sub">{u.email} · {u.roleName} · {u.storeIds.map((id) => s.stores.find((x) => x.id === id)?.name).filter(Boolean).join(', ') || 'без филиалов'}</div>
          </div>
        ))}
      </main>
      {editing && <UserModal user={editing === 'new' ? null : editing} roles={roles.data ?? []} onClose={() => setEditing(null)} onSaved={(m) => { setEditing(null); users.reload(); say(m); }} />}
      {toast}
    </>
  );
}

function UserModal({ user, roles, onClose, onSaved }: { user: StaffUser | null; roles: Role[]; onClose: () => void; onSaved: (msg: string) => void }) {
  const s = useSession();
  const [name, setName] = useState(user?.name ?? '');
  const [email, setEmail] = useState(user?.email ?? '');
  const [password, setPassword] = useState('');
  const [roleId, setRoleId] = useState(user?.roleId ?? roles.find((r) => r.key === 'cashier')?.id ?? roles[0]?.id ?? '');
  const [storeIds, setStoreIds] = useState<string[]>(user?.storeIds ?? (s.storeId ? [s.storeId] : []));
  const [error, setError] = useState<string | null>(null);

  const toggleStore = (id: string) => setStoreIds((cur) => (cur.includes(id) ? cur.filter((x) => x !== id) : [...cur, id]));

  async function save() {
    setError(null);
    try {
      if (user) {
        await api(`/users/${user.id}`, { method: 'PUT', body: { name, roleId, storeIds } });
        if (password) await api(`/users/${user.id}/password`, { body: { password } });
        onSaved('Успешно');
      } else {
        await api('/users', { body: { name, email, password, roleId, storeIds } });
        onSaved('Сотрудник создан');
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Ошибка');
    }
  }

  async function setActive(active: boolean) {
    if (!user) return;
    try {
      await api(`/users/${user.id}`, { method: 'PUT', body: { active } });
      onSaved(active ? 'Активирован' : 'Деактивирован');
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Ошибка');
    }
  }

  return (
    <Modal onClose={onClose}>
      <div className="h2" style={{ marginTop: 0 }}>{user ? 'Сотрудник' : 'Новый сотрудник'}</div>
      <label className="field"><span>Имя</span><input type="text" value={name} onChange={(e) => setName(e.target.value)} /></label>
      <label className="field"><span>Email</span><input type="email" value={email} readOnly={!!user} style={user ? { opacity: 0.6 } : undefined} onChange={(e) => setEmail(e.target.value)} /></label>
      <label className="field"><span>{user ? 'Новый пароль (необязательно)' : 'Пароль'}</span><input type="password" value={password} autoComplete="new-password" onChange={(e) => setPassword(e.target.value)} /></label>
      <div className="sub" style={{ marginTop: -8, marginBottom: 12 }}>Английскими буквами без пробелов, не короче 6 символов</div>
      <label className="field"><span>Роль</span>
        <select value={roleId} onChange={(e) => setRoleId(e.target.value)}>
          {roles.map((r) => <option key={r.id} value={r.id}>{r.name}</option>)}
        </select>
      </label>
      <span className="label">Филиалы</span>
      <div style={{ marginBottom: 16 }}>
        {s.stores.map((st) => (
          <label key={st.id} className="row" style={{ marginBottom: 8 }}>
            <input type="checkbox" checked={storeIds.includes(st.id)} onChange={() => toggleStore(st.id)} style={{ width: 22, height: 22 }} />
            <span>{st.name}</span>
          </label>
        ))}
      </div>
      {error && <div className="error" role="alert">{error}</div>}
      <button className="btn" disabled={!name.trim() || (!user && (!email || password.length < 6)) || (password !== '' && (password.length < 6 || !/^[!-~]+$/.test(password)))} onClick={save}>Сохранить</button>
      {user && user.id !== s.user?.id && (
        <div style={{ marginTop: 10, display: 'grid', gap: 10 }}>
          <button className="btn danger" onClick={() => setActive(false)}>Деактивировать</button>
          <button className="btn light" onClick={() => setActive(true)}>Активировать</button>
        </div>
      )}
    </Modal>
  );
}
