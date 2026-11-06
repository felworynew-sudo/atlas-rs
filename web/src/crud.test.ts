import { describe, expect, it } from 'vitest';
import { buildBody, type Field } from './CrudList';

const fields: Field[] = [
  { key: 'name', label: 'Название', type: 'text' },
  { key: 'params.type', label: 'Тип', type: 'select' },
  { key: 'params.value', label: 'Размер', type: 'number' },
  { key: 'params.delivery', label: 'Доставка', type: 'checkbox' },
  { key: 'endDate', label: 'Окончание', type: 'date', nullable: true },
  { key: 'address', label: 'Адрес', type: 'text' },
];

describe('buildBody', () => {
  it('собирает вложенные поля, числа и флажки', () => {
    const body = buildBody(fields, { name: 'Скидка', 'params.type': 'percent', 'params.value': '12,5', 'params.delivery': true, endDate: '', address: '' });
    expect(body).toEqual({ name: 'Скидка', params: { type: 'percent', value: 12.5, delivery: true }, endDate: null });
  });
  it('пустой необязательный текст пропускается, флажок по умолчанию false', () => {
    const body = buildBody(fields, { name: 'X', 'params.type': 'amount', 'params.value': '', 'params.delivery': false, endDate: '2026-12-31', address: '' });
    expect(body).toEqual({ name: 'X', params: { type: 'amount', delivery: false }, endDate: '2026-12-31' });
  });
});
