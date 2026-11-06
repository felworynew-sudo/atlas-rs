import { describe, expect, it } from 'vitest';
import { change, rub, saleTotals, type CartLine } from './money';

describe('rub', () => {
  it('отрицательный ноль выводится как 0', () => {
    expect(rub(-0)).toBe('0 ₽');
    expect(rub(-0.001)).toBe('0 ₽');
  });
  it('группирует разряды', () => {
    expect(rub(1500).replace(/\s/g, ' ')).toBe('1 500 ₽');
  });
});

const lines: CartLine[] = [
  { key: 'a', name: 'Молоко', price: 100, qty: 2 },
  { key: 'b', name: 'Сыр', price: 250, qty: 1 },
];

describe('saleTotals', () => {
  it('без скидки', () => {
    expect(saleTotals(lines, 0, { type: 'none' })).toEqual({ subtotal: 450, markup: 0, discount: 0, total: 450 });
  });
  it('процентная скидка и надбавка метода покупки', () => {
    const t = saleTotals(lines, 10, { type: 'percent', value: 10 });
    expect(t).toEqual({ subtotal: 450, markup: 45, discount: 45, total: 450 });
  });
  it('скидка суммой не превышает чек', () => {
    expect(saleTotals(lines, 0, { type: 'amount', value: 9999 }).total).toBe(0);
  });
});

describe('change', () => {
  it('считает сдачу', () => expect(change(500, 450)).toBe(50));
  it('недостаточно денег', () => expect(change(100, 450)).toBeNull());
});
