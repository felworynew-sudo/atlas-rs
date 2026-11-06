//! Чистая логика продажи: итоги, скидка, кэшбэк, смешанная оплата, возврат, списание остатков.
use std::collections::HashMap;
use std::hash::Hash;

pub const DEFAULT_CASHBACK_PERCENT: f64 = 3.0;

pub fn round2(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Discount {
    None,
    Percent(f64),
    Amount(f64),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Totals {
    pub subtotal: f64,
    pub markup: f64,
    pub delivery_fee: f64,
    pub discount: f64,
    pub punch_card_reduction: f64,
    pub bonus_reduction: f64,
    pub total: f64,
    pub accumulation: f64,
}

pub struct TotalsInput {
    pub subtotal: f64,
    /// надбавка метода покупки (например доставка), % от суммы позиций
    pub markup_percent: f64,
    pub discount: Discount,
    pub bonus_reduction: f64,
    pub client_balance: f64,
    /// None — клиента нет или кэшбэк выключен
    pub cashback_percent: Option<f64>,
    /// стоимость доставки, оплачиваемая покупателем (добавляется к чеку)
    pub delivery_fee: f64,
    /// скидка по карте «N-й товар в подарок»
    pub punch_card_reduction: f64,
}

/// Карта «N-й товар в подарок»: каждая единица товара добавляет отметку; на N-й единице скидка равна её цене,
/// счётчик обнуляется. Возвращает (скидка, новое значение счётчика).
pub fn punch_card(punches: i32, capacity: i32, unit_prices: &[f64]) -> (f64, i32) {
    let mut p = punches.max(0);
    if capacity < 2 {
        return (0.0, p);
    }
    let mut reduction = 0.0;
    for price in unit_prices {
        p += 1;
        if p >= capacity {
            reduction += price;
            p = 0;
        }
    }
    (round2(reduction), p)
}

/// Итоги чека: сумма позиций + надбавка − скидка − оплата бонусами; кэшбэк начисляется на сумму к оплате.
pub fn totals(i: &TotalsInput) -> Result<Totals, String> {
    if !i.subtotal.is_finite() || i.subtotal < 0.0 {
        return Err("Некорректная сумма".into());
    }
    let subtotal = round2(i.subtotal);
    let markup = round2(subtotal * i.markup_percent.max(0.0) / 100.0);
    let discount = match i.discount {
        Discount::None => 0.0,
        Discount::Percent(p) => {
            if !(0.0..=100.0).contains(&p) {
                return Err("Скидка в процентах от 0 до 100".into());
            }
            round2(subtotal * p / 100.0)
        }
        Discount::Amount(a) => {
            if !a.is_finite() || a < 0.0 || round2(a) > subtotal {
                return Err("Скидка больше суммы чека".into());
            }
            round2(a)
        }
    };
    let bonus = round2(i.bonus_reduction);
    if bonus < 0.0 {
        return Err("Бонусы не могут быть отрицательными".into());
    }
    if bonus > round2(i.client_balance) {
        return Err("Недостаточно бонусов у клиента".into());
    }
    let delivery_fee = round2(i.delivery_fee.max(0.0));
    let punch = round2(i.punch_card_reduction.max(0.0));
    let payable = round2((subtotal + markup + delivery_fee - discount - punch).max(0.0));
    if bonus > payable {
        return Err("Бонусы больше суммы к оплате".into());
    }
    let total = round2(payable - bonus);
    let accumulation = i.cashback_percent.map_or(0.0, |p| round2(total * p.max(0.0) / 100.0));
    Ok(Totals {
        subtotal,
        markup,
        delivery_fee,
        discount,
        punch_card_reduction: punch,
        bonus_reduction: bonus,
        total,
        accumulation,
    })
}

/// Смешанная оплата: два платежа должны покрывать итог до копейки.
pub fn split_ok(total: f64, first: f64, second: f64) -> bool {
    first >= 0.0 && second >= 0.0 && (round2(first + second) - round2(total)).abs() < 0.005
}

/// Сумма возврата строк `(qty, price)` пропорционально доле итога в сумме позиций (учёт скидки и бонусов).
pub fn refund_amount(sale_subtotal: f64, sale_total: f64, lines: &[(f64, f64)]) -> f64 {
    if sale_subtotal <= 0.0 {
        return 0.0;
    }
    let gross: f64 = lines.iter().map(|(q, p)| q * p).sum();
    round2(gross * sale_total / sale_subtotal)
}

/// Узел для расчёта списания остатков.
#[derive(Debug, Clone)]
pub struct StockNode<K> {
    pub service: bool,
    /// составной «производится во время продажи» — остаток 0, списываются ингредиенты
    pub on_sale_composite: bool,
    pub ingredients: Vec<(K, f64)>,
}

fn expand<K: Copy + Eq + Hash>(nodes: &HashMap<K, StockNode<K>>, id: K, qty: f64, depth: usize, out: &mut Vec<(K, f64)>) {
    if depth > 16 {
        return;
    }
    let Some(n) = nodes.get(&id) else { return };
    if n.service {
        return;
    }
    if n.on_sale_composite {
        for (i, q) in &n.ingredients {
            expand(nodes, *i, qty * q, depth + 1, out);
        }
    } else if let Some(e) = out.iter_mut().find(|(k, _)| *k == id) {
        e.1 += qty;
    } else {
        out.push((id, qty));
    }
}

/// На какие товары и сколько реально влияет продажа `qty` единиц `id` (услуги — ни на какие).
pub fn stock_effects<K: Copy + Eq + Hash>(nodes: &HashMap<K, StockNode<K>>, id: K, qty: f64) -> Vec<(K, f64)> {
    let mut out = Vec::new();
    expand(nodes, id, qty, 0, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inp(subtotal: f64) -> TotalsInput {
        TotalsInput {
            subtotal,
            markup_percent: 0.0,
            discount: Discount::None,
            bonus_reduction: 0.0,
            client_balance: 0.0,
            cashback_percent: None,
            delivery_fee: 0.0,
            punch_card_reduction: 0.0,
        }
    }

    #[test]
    fn punch_card_every_nth_unit_is_free() {
        // капасити 3: счётчик 1 → 2-я единица ещё платная, 3-я (цена 30) бесплатна, 4-я — новая отметка
        assert_eq!(punch_card(1, 3, &[10.0, 30.0, 50.0]), (30.0, 1));
        assert_eq!(punch_card(0, 3, &[10.0, 20.0]), (0.0, 2));
        // две подарочные единицы за одну продажу
        assert_eq!(punch_card(2, 2, &[5.0, 7.0, 9.0]), (5.0 + 9.0, 0));
        // выключенная карта (ёмкость < 2) ничего не делает
        assert_eq!(punch_card(4, 1, &[10.0]), (0.0, 4));
    }

    #[test]
    fn delivery_fee_and_punch_card_in_totals() {
        let t = totals(&TotalsInput { delivery_fee: 200.0, punch_card_reduction: 100.0, ..inp(1000.0) }).unwrap();
        assert_eq!(t.total, 1100.0);
        assert_eq!(t.delivery_fee, 200.0);
    }

    #[test]
    fn percent_discount_and_cashback() {
        let t = totals(&TotalsInput {
            discount: Discount::Percent(10.0),
            cashback_percent: Some(DEFAULT_CASHBACK_PERCENT),
            ..inp(1000.0)
        })
        .unwrap();
        assert_eq!(t.discount, 100.0);
        assert_eq!(t.total, 900.0);
        assert_eq!(t.accumulation, 27.0);
    }

    #[test]
    fn bonus_payment_and_markup() {
        let t = totals(&TotalsInput {
            markup_percent: 10.0,
            discount: Discount::Amount(50.0),
            bonus_reduction: 100.0,
            client_balance: 150.0,
            cashback_percent: Some(1.0),
            ..inp(1000.0)
        })
        .unwrap();
        // 1000 + 100 - 50 - 100 = 950; кэшбэк 1% = 9.5
        assert_eq!(t.total, 950.0);
        assert_eq!(t.accumulation, 9.5);
    }

    #[test]
    fn errors() {
        assert!(totals(&TotalsInput { discount: Discount::Amount(2000.0), ..inp(1000.0) }).is_err());
        assert!(totals(&TotalsInput { discount: Discount::Percent(101.0), ..inp(1000.0) }).is_err());
        assert!(totals(&TotalsInput { bonus_reduction: 10.0, client_balance: 5.0, ..inp(1000.0) }).is_err());
        assert!(totals(&inp(-1.0)).is_err());
    }

    #[test]
    fn split_and_refund() {
        assert!(split_ok(900.0, 500.0, 400.0));
        assert!(!split_ok(900.0, 500.0, 300.0));
        // продажа 1000, итог 900 (скидка 10%): возврат позиции на 500 → 450
        assert_eq!(refund_amount(1000.0, 900.0, &[(1.0, 500.0)]), 450.0);
    }

    #[test]
    fn stock_effects_expand_on_sale_composite() {
        let mut m = HashMap::new();
        m.insert(1, StockNode { service: false, on_sale_composite: false, ingredients: vec![] });
        m.insert(2, StockNode { service: true, on_sale_composite: false, ingredients: vec![] });
        m.insert(3, StockNode { service: false, on_sale_composite: true, ingredients: vec![(1, 0.2), (2, 1.0)] });
        m.insert(4, StockNode { service: false, on_sale_composite: false, ingredients: vec![(1, 5.0)] }); // «заранее»
        assert_eq!(stock_effects(&m, 1, 2.0), vec![(1, 2.0)]);
        assert!(stock_effects(&m, 2, 1.0).is_empty());
        let e = stock_effects(&m, 3, 10.0);
        assert_eq!(e.len(), 1);
        assert!((e[0].1 - 2.0).abs() < 1e-9);
        assert_eq!(stock_effects(&m, 4, 1.0), vec![(4, 1.0)]);
    }
}
