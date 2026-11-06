import { describe, expect, it } from 'vitest';
import { buildUrl } from './api';

describe('buildUrl', () => {
  it('добавляет префикс /api и пропускает пустые параметры', () => {
    expect(buildUrl('/stocks', { storeId: 'abc', q: '', n: undefined, flag: false })).toBe('/api/stocks?storeId=abc&flag=false');
  });
  it('без параметров — без «?»', () => {
    expect(buildUrl('/stores')).toBe('/api/stores');
  });
});
