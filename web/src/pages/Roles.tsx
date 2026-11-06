import { useMemo, useState } from 'react';
import { api, type PermissionDef, type Role } from '../api';
import { Icon } from '../Icon';
import { rub } from '../money';
import { Header, Modal, Spinner, useLoad, useToast } from '../ui';

export const MODULE_TITLES: Record<string, string> = {
  simple_sales: 'Простые продажи',
  complex_sales: 'Комплексные продажи',
  spendings: 'Расходы',
  shifts: 'Смена',
  stocks: 'Остатки товаров',
  supplies: 'Приемки',
  writeoffs: 'Списания',
  revisions: 'Ревизии',
  transfers: 'Перемещения',
  salary: 'Зарплата',
  checklist: 'Чеклист',
  marketplace: 'Онлайн каталог и заказы',
  updates: 'Обновления',
  payments: 'Платежи',
  label_printing: 'Печать этикеток',
  manufacture: 'Производство',
  clients: 'Клиенты и Программа лояльности',
};

const SALARY = [['none', 'Нет'], ['hour', 'За час'], ['day', 'За день / смену'], ['month', 'За месяц']] as const;
const BONUS = [['none', 'Нет'], ['percent_own_sales', 'Процент от индивидуальных продаж']] as const;

/** Роли и доступы: список ролей и редактор прав по модулям. */
export default function Roles() {
  const [toast, say] = useToast();
  const roles = useLoad(() => api<Role[]>('/roles'), []);
  const perms = useLoad(() => api<PermissionDef[]>('/permissions'), []);
  const [editing, setEditing] = useState<Role | 'new' | null>(null);

  return (
    <>
      <Header title="Роли и доступы" right={<button className="icon-btn" aria-label="Новая роль" onClick={() => setEditing('new')}><Icon name="plus" /></button>} />
      <main className="page">
        {roles.loading && <Spinner />}
        {(roles.data ?? []).map((r) => (
          <div key={r.id} className="card click" onClick={() => setEditing(r)}>
            <div className="title">{r.name}</div>
            <div className="sub">{r.key === 'admin' ? 'Все права' : `${r.permissions.length} прав`}{r.salaryType !== 'none' ? ` · ставка ${rub(r.salaryRate)}` : ''}</div>
          </div>
        ))}
      </main>
      {editing && <RoleModal role={editing === 'new' ? null : editing} perms={perms.data ?? []} onClose={() => setEditing(null)} onSaved={() => { setEditing(null); roles.reload(); say('Успешно'); }} />}
      {toast}
    </>
  );
}

function RoleModal({ role, perms, onClose, onSaved }: { role: Role | null; perms: PermissionDef[]; onClose: () => void; onSaved: () => void }) {
  const isAdmin = role?.key === 'admin';
  const [name, setName] = useState(role?.name ?? '');
  const [salaryType, setSalaryType] = useState<string>(role?.salaryType ?? 'none');
  const [salaryRate, setSalaryRate] = useState(String(role?.salaryRate ?? 0));
  const [bonusType, setBonusType] = useState<string>(role?.bonusType ?? 'none');
  const [bonusRate, setBonusRate] = useState(String(role?.bonusRate ?? 0));
  const [selected, setSelected] = useState<Set<string>>(new Set(role?.permissions ?? []));
  const [error, setError] = useState<string | null>(null);

  const groups = useMemo(() => {
    const m = new Map<string, PermissionDef[]>();
    for (const p of perms) {
      const g = p.module ? MODULE_TITLES[p.module] ?? p.module : 'Основные';
      m.set(g, [...(m.get(g) ?? []), p]);
    }
    return [...m.entries()];
  }, [perms]);

  const toggle = (key: string) => setSelected((cur) => { const n = new Set(cur); if (n.has(key)) n.delete(key); else n.add(key); return n; });

  async function save() {
    setError(null);
    const body = {
      name,
      salaryType,
      salaryRate: Number(salaryRate.replace(',', '.')) || 0,
      bonusType,
      bonusRate: Number(bonusRate.replace(',', '.')) || 0,
      permissions: isAdmin ? undefined : [...selected],
    };
    try {
      if (role) await api(`/roles/${role.id}`, { method: 'PUT', body });
      else await api('/roles', { body });
      onSaved();
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Ошибка');
    }
  }

  async function remove() {
    if (!role || !confirm('Удалить роль?')) return;
    try {
      await api(`/roles/${role.id}`, { method: 'DELETE' });
      onSaved();
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Ошибка');
    }
  }

  return (
    <Modal onClose={onClose}>
      <div className="h2" style={{ marginTop: 0 }}>{role ? 'Роль' : 'Новая роль'}</div>
      <label className="field"><span>Название</span><input type="text" value={name} onChange={(e) => setName(e.target.value)} /></label>
      <label className="field"><span>Тип зарплаты</span>
        <select value={salaryType} onChange={(e) => setSalaryType(e.target.value)}>{SALARY.map(([v, l]) => <option key={v} value={v}>{l}</option>)}</select>
      </label>
      {salaryType !== 'none' && <label className="field"><span>Ставка</span><input type="number" min="0" step="any" value={salaryRate} onChange={(e) => setSalaryRate(e.target.value)} /></label>}
      <label className="field"><span>Тип бонусов</span>
        <select value={bonusType} onChange={(e) => setBonusType(e.target.value)}>{BONUS.map(([v, l]) => <option key={v} value={v}>{l}</option>)}</select>
      </label>
      {bonusType !== 'none' && <label className="field"><span>Процент бонуса</span><input type="number" min="0" max="100" step="any" value={bonusRate} onChange={(e) => setBonusRate(e.target.value)} /></label>}

      <div className="h2" style={{ fontSize: 18 }}>Права</div>
      {isAdmin && <div className="muted" style={{ marginBottom: 12 }}>У роли «Админ» все права, изменить их нельзя.</div>}
      {groups.map(([group, list]) => (
        <section key={group} style={{ marginBottom: 14 }}>
          <div className="day" style={{ marginTop: 8 }}>{group}</div>
          {list.map((p) => (
            <label key={p.key} className="row" style={{ marginBottom: 8 }}>
              <input type="checkbox" disabled={isAdmin} checked={isAdmin || selected.has(p.key)} onChange={() => toggle(p.key)} style={{ width: 22, height: 22 }} />
              <span style={{ fontSize: 13 }}>{p.title}</span>
            </label>
          ))}
        </section>
      ))}
      {error && <div className="error" role="alert">{error}</div>}
      <button className="btn" disabled={!name.trim()} onClick={save}>Сохранить</button>
      {role && !['admin', 'cashier'].includes(role.key) && <button className="btn danger" style={{ marginTop: 10 }} onClick={remove}>Удалить</button>}
    </Modal>
  );
}
