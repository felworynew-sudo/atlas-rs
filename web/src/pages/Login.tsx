import { useState, type FormEvent } from 'react';
import { useSession } from '../session';

export default function Login() {
  const { login, signup } = useSession();
  const [mode, setMode] = useState<'login' | 'signup'>('login');
  const [f, setF] = useState({ email: '', password: '', name: '', organizationName: '', storeName: '' });
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const set = (k: keyof typeof f) => (e: React.ChangeEvent<HTMLInputElement>) => setF({ ...f, [k]: e.target.value });

  async function submit(e: FormEvent) {
    e.preventDefault();
    setError(null);
    setBusy(true);
    try {
      if (mode === 'login') await login(f.email, f.password);
      else await signup({ email: f.email, password: f.password, name: f.name, organizationName: f.organizationName, storeName: f.storeName || undefined });
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Ошибка');
    } finally {
      setBusy(false);
    }
  }

  return (
    <form className="auth" onSubmit={submit}>
      <h1>{mode === 'login' ? 'Вход в Атлас' : 'Регистрация'}</h1>
      {mode === 'signup' && (
        <>
          <label className="field"><span>Ваше имя</span><input type="text" value={f.name} onChange={set('name')} autoComplete="name" /></label>
          <label className="field"><span>Название компании</span><input type="text" value={f.organizationName} onChange={set('organizationName')} /></label>
          <label className="field"><span>Название филиала</span><input type="text" value={f.storeName} onChange={set('storeName')} placeholder="Филиал 1" /></label>
        </>
      )}
      <label className="field"><span>Email</span><input type="email" value={f.email} onChange={set('email')} autoComplete="email" /></label>
      <label className="field"><span>Пароль</span><input type="password" value={f.password} onChange={set('password')} autoComplete={mode === 'login' ? 'current-password' : 'new-password'} /></label>
      {error && <div className="error" role="alert">{error}</div>}
      <button className="btn" disabled={busy}>{mode === 'login' ? 'Войти' : 'Создать'}</button>
      <p style={{ textAlign: 'center', marginTop: 20 }}>
        <button type="button" className="link" onClick={() => { setMode(mode === 'login' ? 'signup' : 'login'); setError(null); }}>
          {mode === 'login' ? 'Зарегистрировать компанию' : 'У меня уже есть аккаунт'}
        </button>
      </p>
    </form>
  );
}
