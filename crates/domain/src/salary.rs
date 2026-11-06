//! Касса смены и зарплата: расчётные формулы (чистая логика).
use crate::sale::round2;

/// Составляющие расчётного остатка наличных в кассе.
#[derive(Debug, Clone, Copy, Default)]
pub struct CashInput {
    pub opening: f64,
    pub cash_sales: f64,
    pub cash_returns: f64,
    pub withdrawals: f64,
    pub cash_spendings: f64,
    pub paid_supplies: f64,
    pub deposits: f64,
}

/// Итог наличных = начало + продажи наличными − возвраты наличными − изъятия − расходы наличными − оплаченные приёмки + внесения.
pub fn expected_cash(i: &CashInput) -> f64 {
    round2(i.opening + i.cash_sales - i.cash_returns - i.withdrawals - i.cash_spendings - i.paid_supplies + i.deposits)
}

/// Недосдача (< 0) или излишек (> 0) = факт − расчёт.
pub fn cash_difference(actual: f64, expected: f64) -> f64 {
    round2(actual - expected)
}

/// Базовая оплата: «за час» — часы × ставка; «за день/смену» — ставка × число смен; «за месяц» — доля месяца за период.
pub fn base_pay(salary_type: &str, rate: f64, hours: f64, shifts: i64, month_share: f64) -> f64 {
    match salary_type {
        "hour" => round2(hours * rate),
        "day" => round2(rate * shifts as f64),
        "month" => round2(month_share),
        _ => 0.0,
    }
}

/// Бонус «процент от индивидуальных продаж» = % × (продажи сотрудника − возвраты).
pub fn bonus_pay(bonus_type: &str, percent: f64, own_sales_net: f64) -> f64 {
    if bonus_type == "percent_own_sales" {
        round2(percent * own_sales_net.max(0.0) / 100.0)
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cash_formula() {
        let e = expected_cash(&CashInput {
            opening: 1000.0,
            cash_sales: 5000.0,
            cash_returns: 300.0,
            withdrawals: 2000.0,
            cash_spendings: 450.0,
            paid_supplies: 700.0,
            deposits: 100.0,
        });
        assert_eq!(e, 2650.0);
        assert_eq!(cash_difference(2600.0, e), -50.0);
        assert_eq!(cash_difference(2700.0, e), 50.0);
    }

    #[test]
    fn pay_types() {
        assert_eq!(base_pay("hour", 300.0, 7.5, 2, 0.0), 2250.0);
        assert_eq!(base_pay("day", 2000.0, 99.0, 3, 0.0), 6000.0);
        assert_eq!(base_pay("month", 60000.0, 0.0, 0, 30000.0), 30000.0);
        assert_eq!(base_pay("none", 500.0, 10.0, 10, 500.0), 0.0);
        assert_eq!(bonus_pay("percent_own_sales", 5.0, 10000.0), 500.0);
        assert_eq!(bonus_pay("none", 5.0, 10000.0), 0.0);
        assert_eq!(bonus_pay("percent_own_sales", 5.0, -100.0), 0.0);
    }
}
