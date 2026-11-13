// Где живёт интерфейс: '' — в корне (отдельный сайт), '/crm-app' — внутри дев-панели сайта студии (сборка с ATLAS_BASE).
export const BASE = (import.meta.env.BASE_URL || '/').replace(/\/$/, '');

/** Открыт во вкладке «CRM» дев-панели (в рамке): вход и выход — через сайт. */
export const EMBEDDED = (() => {
  try {
    return window.self !== window.top;
  } catch {
    return true;
  }
})();

/** Попросить дев-панель выдать новый вход (токен истёк или сотрудник вышел). */
export function askSiteLogin(): void {
  if (!EMBEDDED) return;
  try {
    window.parent.postMessage({ type: 'atlas:login-required' }, window.location.origin);
  } catch {
    /* рамка другого сайта — ничего не делаем */
  }
}
