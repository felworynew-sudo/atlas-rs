import { createContext, useCallback, useContext, useEffect, useMemo, useState, type ReactNode } from 'react';
import { setCurrency } from './money';
import { api, getToken, setToken, type Org, type Store, type UserView } from './api';

interface Session {
  ready: boolean;
  user: UserView | null;
  org: Org | null;
  stores: Store[];
  storeId: string | null;
  setStoreId: (id: string) => void;
  can: (permission: string) => boolean;
  moduleOn: (module: string) => boolean;
  login: (email: string, password: string) => Promise<void>;
  signup: (dto: { email: string; password: string; name: string; organizationName: string; storeName?: string }) => Promise<void>;
  logout: () => void;
  /** Перечитать пользователя, организацию и филиалы (после изменения настроек). */
  reload: () => Promise<void>;
}

const Ctx = createContext<Session | null>(null);

export function useSession(): Session {
  const s = useContext(Ctx);
  if (!s) throw new Error('SessionProvider не найден');
  return s;
}

const STORE_KEY = 'atlas.store';

export function SessionProvider({ children }: { children: ReactNode }) {
  const [ready, setReady] = useState(false);
  const [user, setUser] = useState<UserView | null>(null);
  const [org, setOrg] = useState<Org | null>(null);
  const [stores, setStores] = useState<Store[]>([]);
  const [storeId, setStoreIdState] = useState<string | null>(null);

  const load = useCallback(async () => {
    const [me, o, st] = await Promise.all([api<UserView>('/auth/me'), api<Org>('/organizations'), api<Store[]>('/stores')]);
    const mine = st.filter((s) => me.roleKey === 'admin' || me.permissions.includes('settings.other_stores') || me.storeIds.includes(s.id));
    setCurrency(o.currency ?? 'RUB', o.priceDecimals ?? 2);
    setUser(me);
    setOrg(o);
    setStores(mine);
    let saved: string | null = null;
    try {
      saved = localStorage.getItem(STORE_KEY);
    } catch {
      /* без хранилища */
    }
    setStoreIdState(mine.find((s) => s.id === saved)?.id ?? mine[0]?.id ?? null);
  }, []);

  const logout = useCallback(() => {
    setToken(null);
    setUser(null);
    setOrg(null);
    setStores([]);
    setStoreIdState(null);
  }, []);

  useEffect(() => {
    // Вход с корпоративного сайта: сайт передаёт токен в адресе вида /#sso=<токен>; сразу убираем его из адреса.
    const m = /[#&]sso=([^&]+)/.exec(window.location.hash);
    if (m) {
      setToken(decodeURIComponent(m[1]));
      window.history.replaceState(null, '', window.location.pathname + window.location.search);
    }
    if (!getToken()) {
      setReady(true);
      return;
    }
    load().catch(logout).finally(() => setReady(true));
  }, [load, logout]);

  useEffect(() => {
    window.addEventListener('atlas:unauthorized', logout);
    return () => window.removeEventListener('atlas:unauthorized', logout);
  }, [logout]);

  const value = useMemo<Session>(
    () => ({
      ready,
      user,
      org,
      stores,
      storeId,
      setStoreId: (id) => {
        setStoreIdState(id);
        try {
          localStorage.setItem(STORE_KEY, id);
        } catch {
          /* без хранилища */
        }
      },
      can: (p) => !!user && user.permissions.includes(p),
      moduleOn: (m) => !!org && org.modules[m] === true,
      login: async (email, password) => {
        const r = await api<{ token: string }>('/auth/login', { body: { email, password } });
        setToken(r.token);
        await load();
      },
      signup: async (dto) => {
        const r = await api<{ token: string }>('/auth/signup', { body: dto });
        setToken(r.token);
        await load();
      },
      logout,
      reload: load,
    }),
    [ready, user, org, stores, storeId, load, logout],
  );

  return <Ctx.Provider value={value}>{children}</Ctx.Provider>;
}
