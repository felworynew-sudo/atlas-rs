import { useState, type FormEvent } from 'react';
import { api } from './api';
import { Icon } from './Icon';
import { Header, Modal, Spinner, useLoad, useToast } from './ui';

export interface Field {
  /** путь в объекте записи; точка означает вложенность (`params.kind`) */
  key: string;
  label: string;
  type: 'text' | 'number' | 'select' | 'checkbox' | 'date';
  options?: { value: string; label: string }[];
  placeholder?: string;
  required?: boolean;
  /** пустое значение отправляется как null (иначе поле пропускается) */
  nullable?: boolean;
  defaultValue?: string | boolean;
}

export type Row = { id: string; name?: string } & Record<string, unknown>;

export interface CrudConfig {
  title: string;
  /** путь коллекции: GET, POST создаёт, `${path}/${id}` — PUT и DELETE */
  path: string;
  fields: Field[];
  subtitle?: (r: Row) => string;
  /** кто может менять (иначе только просмотр) */
  canEdit: boolean;
  deletable?: boolean;
}

const getPath = (o: unknown, path: string): unknown => path.split('.').reduce<unknown>((acc, k) => (acc && typeof acc === 'object' ? (acc as Record<string, unknown>)[k] : undefined), o);

function setPath(target: Record<string, unknown>, path: string, value: unknown) {
  const keys = path.split('.');
  let cur = target;
  keys.slice(0, -1).forEach((k) => {
    cur = (cur[k] ??= {}) as Record<string, unknown>;
  });
  cur[keys[keys.length - 1]] = value;
}

/** Форма значений → тело запроса (числа разбираются, пустые необязательные поля пропускаются). */
export function buildBody(fields: Field[], values: Record<string, string | boolean>): Record<string, unknown> {
  const body: Record<string, unknown> = {};
  for (const f of fields) {
    const v = values[f.key];
    if (f.type === 'checkbox') setPath(body, f.key, v === true);
    else if (v === '' || v === undefined) {
      if (f.nullable) setPath(body, f.key, null);
    } else if (f.type === 'number') setPath(body, f.key, Number(String(v).replace(',', '.')));
    else setPath(body, f.key, v);
  }
  return body;
}

function initial(fields: Field[], row: Row | null): Record<string, string | boolean> {
  const out: Record<string, string | boolean> = {};
  for (const f of fields) {
    const raw = row ? getPath(row, f.key) : f.defaultValue;
    out[f.key] = f.type === 'checkbox' ? raw === true : raw === undefined || raw === null ? '' : String(raw);
  }
  return out;
}

/** Универсальный экран списка настроек: список записей, модал создания и редактирования, удаление. */
export function CrudList({ config }: { config: CrudConfig }) {
  const [toast, say] = useToast();
  const list = useLoad(() => api<Row[]>(config.path), [config.path]);
  const [editing, setEditing] = useState<Row | 'new' | null>(null);

  return (
    <>
      <Header title={config.title} right={config.canEdit ? <button className="icon-btn" aria-label="Добавить" onClick={() => setEditing('new')}><Icon name="plus" /></button> : undefined} />
      <main className="page">
        {list.loading && <Spinner />}
        {list.error && <div className="error">{list.error}</div>}
        {!list.loading && (list.data ?? []).length === 0 && <div className="empty">Список пуст</div>}
        {(list.data ?? []).map((r) => (
          <div key={r.id} className={`card${config.canEdit ? ' click' : ''}`} onClick={() => config.canEdit && setEditing(r)}>
            <div className="title">{r.name}</div>
            {config.subtitle && <div className="sub">{config.subtitle(r)}</div>}
          </div>
        ))}
      </main>
      {editing && (
        <EditModal
          config={config}
          row={editing === 'new' ? null : editing}
          onClose={() => setEditing(null)}
          onSaved={() => { setEditing(null); list.reload(); say('Успешно'); }}
        />
      )}
      {toast}
    </>
  );
}

function EditModal({ config, row, onClose, onSaved }: { config: CrudConfig; row: Row | null; onClose: () => void; onSaved: () => void }) {
  const [values, setValues] = useState(() => initial(config.fields, row));
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const missing = config.fields.some((f) => f.required && values[f.key] === '');

  async function submit(e: FormEvent) {
    e.preventDefault();
    setBusy(true);
    setError(null);
    try {
      const body = buildBody(config.fields, values);
      if (row) await api(`${config.path}/${row.id}`, { method: 'PUT', body });
      else await api(config.path, { body });
      onSaved();
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Ошибка');
    } finally {
      setBusy(false);
    }
  }

  async function remove() {
    if (!row || !confirm('Удалить запись?')) return;
    try {
      await api(`${config.path}/${row.id}`, { method: 'DELETE' });
      onSaved();
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Ошибка');
    }
  }

  return (
    <Modal onClose={onClose}>
      <form onSubmit={submit}>
        <div className="h2" style={{ marginTop: 0 }}>{row ? 'Редактирование' : 'Новая запись'}</div>
        {config.fields.map((f) =>
          f.type === 'checkbox' ? (
            <label key={f.key} className="row" style={{ marginBottom: 18 }}>
              <input type="checkbox" checked={values[f.key] === true} onChange={(e) => setValues({ ...values, [f.key]: e.target.checked })} style={{ width: 22, height: 22 }} />
              <span>{f.label}</span>
            </label>
          ) : (
            <label key={f.key} className="field">
              <span>{f.label}</span>
              {f.type === 'select' ? (
                <select value={String(values[f.key])} onChange={(e) => setValues({ ...values, [f.key]: e.target.value })}>
                  {f.options?.map((o) => <option key={o.value} value={o.value}>{o.label}</option>)}
                </select>
              ) : (
                <input type={f.type} step={f.type === 'number' ? 'any' : undefined} value={String(values[f.key])} placeholder={f.placeholder} onChange={(e) => setValues({ ...values, [f.key]: e.target.value })} />
              )}
            </label>
          ),
        )}
        {error && <div className="error" role="alert">{error}</div>}
        <button className="btn" disabled={busy || missing}>Сохранить</button>
        {row && config.deletable !== false && <button type="button" className="btn danger" style={{ marginTop: 10 }} onClick={remove}>Удалить</button>}
      </form>
    </Modal>
  );
}
