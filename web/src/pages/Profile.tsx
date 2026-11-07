import { useState } from 'react';
import { api } from '../api';
import { useSession } from '../session';
import { Header, useToast } from '../ui';

/** Профиль: данные сотрудника, смена своего пароля, выход. */
export default function Profile() {
  const s = useSession();
  const [toast, say] = useToast();
  const [pw, setPw] = useState('');
  const [pw2, setPw2] = useState('');
  const [error, setError] = useState<string | null>(null);

  async function save() {
    setError(null);
    try {
      await api(`/users/${s.user?.id}/password`, { body: { password: pw } });
      setPw('');
      setPw2('');
      say('Пароль изменён');
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Ошибка');
    }
  }

  return (
    <>
      <Header title="Профиль" />
      <main className="page">
        <div className="card">
          <div className="title">{s.user?.name}</div>
          <div className="sub">{s.user?.email} · {s.user?.roleName}</div>
          <div className="sub">{s.org?.name}</div>
        </div>
        <div className="h2">Смена пароля</div>
        <label className="field"><span>Новый пароль</span><input type="password" autoComplete="new-password" value={pw} onChange={(e) => setPw(e.target.value)} /></label>
        <label className="field"><span>Повторите пароль</span><input type="password" autoComplete="new-password" value={pw2} onChange={(e) => setPw2(e.target.value)} /></label>
        {error && <div className="error" role="alert">{error}</div>}
        <button className="btn" disabled={pw.length < 6 || pw !== pw2} onClick={save}>Сохранить пароль</button>
        <button className="btn light" style={{ marginTop: 16 }} onClick={() => confirm('Выйти из аккаунта?') && s.logout()}>Выйти</button>
      </main>
      {toast}
    </>
  );
}
