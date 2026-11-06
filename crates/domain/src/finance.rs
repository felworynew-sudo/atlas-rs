//! Финансовая логика: доля ежемесячных расходов и отчёт о прибылях и убытках (P&L).
use crate::sale::round2;
use chrono::{Datelike, NaiveDate};

pub fn days_in_month(year: i32, month: u32) -> u32 {
    let (ny, nm) = if month == 12 { (year + 1, 1) } else { (year, month + 1) };
    let first = NaiveDate::from_ymd_opt(year, month, 1).expect("valid month");
    let next = NaiveDate::from_ymd_opt(ny, nm, 1).expect("valid month");
    (next - first).num_days() as u32
}

/// Доля ежемесячного расхода за период `[from, to]` (включительно): каждый день — сумма / число дней его месяца.
/// Учитываются только дни внутри `[start, end]`. Пример: 100 000 ₽ в месяце из 30 дней → 3 333,33 ₽ в день.
pub fn monthly_share(amount: f64, start: NaiveDate, end: Option<NaiveDate>, from: NaiveDate, to: NaiveDate) -> f64 {
    let first = from.max(start);
    let last = end.map_or(to, |e| e.min(to));
    if last < first {
        return 0.0;
    }
    let mut total = 0.0;
    let mut d = first;
    while d <= last {
        total += amount / f64::from(days_in_month(d.year(), d.month()));
        d = d.succ_opt().expect("date in range");
    }
    round2(total)
}

/// Входные данные P&L за период (все суммы уже агрегированы).
#[derive(Debug, Clone, Default)]
pub struct PnlInput {
    /// выручка до оплаты бонусами: Σ (итог чека + оплачено бонусами)
    pub sales: f64,
    pub cost_of_sales: f64,
    pub paid_by_bonus: f64,
    pub returns: f64,
    pub writeoffs: f64,
    /// Σ разниц ревизий со знаком: недостача < 0, излишек > 0
    pub revision_diff: f64,
    /// Σ разниц закрытия смен со знаком
    pub shift_diff: f64,
    pub salaries: f64,
    pub bank_commission: f64,
    pub operating_expenses: f64,
    pub monthly_expenses: f64,
    pub capital_expenses: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Pnl {
    /// Маржа (Gross Income) = продажи − себестоимость − бонусы
    pub gross_income: f64,
    /// Операционная прибыль (Net Income)
    pub operating_profit: f64,
    /// Чистая прибыль = операционная − капитальные вложения
    pub net_profit: f64,
}

pub fn pnl(i: &PnlInput) -> Pnl {
    let gross = round2(i.sales - i.cost_of_sales - i.paid_by_bonus);
    let operating = round2(
        gross - i.returns - i.writeoffs + i.revision_diff + i.shift_diff - i.salaries - i.bank_commission
            - i.operating_expenses
            - i.monthly_expenses,
    );
    Pnl { gross_income: gross, operating_profit: operating, net_profit: round2(operating - i.capital_expenses) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    #[test]
    fn month_lengths() {
        assert_eq!(days_in_month(2026, 2), 28);
        assert_eq!(days_in_month(2028, 2), 29);
        assert_eq!(days_in_month(2026, 12), 31);
        assert_eq!(days_in_month(2026, 11), 30);
    }

    #[test]
    fn monthly_share_per_day_and_whole_month() {
        // ноябрь 2026 — 30 дней: 100 000 / 30 = 3 333,33 в день
        assert_eq!(monthly_share(100_000.0, d(2026, 1, 1), None, d(2026, 11, 6), d(2026, 11, 6)), 3333.33);
        assert_eq!(monthly_share(100_000.0, d(2026, 1, 1), None, d(2026, 11, 1), d(2026, 11, 30)), 100_000.0);
    }

    #[test]
    fn monthly_share_respects_start_and_end() {
        // с 16 по 30 ноября — ровно половина
        assert_eq!(monthly_share(30_000.0, d(2026, 11, 16), None, d(2026, 11, 1), d(2026, 11, 30)), 15_000.0);
        // закончился 10 ноября: 10 дней
        assert_eq!(monthly_share(30_000.0, d(2026, 1, 1), Some(d(2026, 11, 10)), d(2026, 11, 1), d(2026, 11, 30)), 10_000.0);
        // период до начала расхода
        assert_eq!(monthly_share(30_000.0, d(2026, 12, 1), None, d(2026, 11, 1), d(2026, 11, 30)), 0.0);
        // два месяца с разной длиной: октябрь 31 + ноябрь 30 дней
        assert_eq!(monthly_share(310.0 * 3.0, d(2026, 1, 1), None, d(2026, 10, 1), d(2026, 11, 30)), 1860.0);
    }

    #[test]
    fn pnl_structure() {
        let p = pnl(&PnlInput {
            sales: 10_000.0,
            cost_of_sales: 6_000.0,
            paid_by_bonus: 200.0,
            returns: 500.0,
            writeoffs: 300.0,
            revision_diff: -100.0,
            salaries: 1_000.0,
            bank_commission: 50.0,
            operating_expenses: 400.0,
            monthly_expenses: 3_333.33,
            capital_expenses: 1_000.0,
            ..Default::default()
        });
        assert_eq!(p.gross_income, 3_800.0);
        // 3800 − 500 − 300 − 100 − 1000 − 50 − 400 − 3333.33 = −1883.33
        assert_eq!(p.operating_profit, -1_883.33);
        assert_eq!(p.net_profit, -2_883.33);
    }
}
