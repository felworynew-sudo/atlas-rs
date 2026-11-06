//! Модули организации (Настройки организации → «Используемые модули»).
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Module {
    SimpleSales,
    ComplexSales,
    Spendings,
    Shifts,
    Stocks,
    Supplies,
    Writeoffs,
    Revisions,
    Transfers,
    Salary,
    Checklist,
    Marketplace,
    Updates,
    Payments,
    LabelPrinting,
    Manufacture,
    Clients,
}

impl Module {
    pub const ALL: [Module; 17] = [
        Module::SimpleSales,
        Module::ComplexSales,
        Module::Spendings,
        Module::Shifts,
        Module::Stocks,
        Module::Supplies,
        Module::Writeoffs,
        Module::Revisions,
        Module::Transfers,
        Module::Salary,
        Module::Checklist,
        Module::Marketplace,
        Module::Updates,
        Module::Payments,
        Module::LabelPrinting,
        Module::Manufacture,
        Module::Clients,
    ];

    /// Включено по умолчанию в новой организации (как в эталонной организации Атласа).
    pub fn default_enabled(self) -> bool {
        matches!(
            self,
            Module::SimpleSales
                | Module::ComplexSales
                | Module::Spendings
                | Module::Stocks
                | Module::Writeoffs
                | Module::Revisions
                | Module::Marketplace
                | Module::Clients
        )
    }

    /// Ключ модуля в JSON организации (`snake_case`).
    pub fn key(self) -> &'static str {
        match self {
            Module::SimpleSales => "simple_sales",
            Module::ComplexSales => "complex_sales",
            Module::Spendings => "spendings",
            Module::Shifts => "shifts",
            Module::Stocks => "stocks",
            Module::Supplies => "supplies",
            Module::Writeoffs => "writeoffs",
            Module::Revisions => "revisions",
            Module::Transfers => "transfers",
            Module::Salary => "salary",
            Module::Checklist => "checklist",
            Module::Marketplace => "marketplace",
            Module::Updates => "updates",
            Module::Payments => "payments",
            Module::LabelPrinting => "label_printing",
            Module::Manufacture => "manufacture",
            Module::Clients => "clients",
        }
    }

    /// Русское название модуля (для UI).
    pub fn title(self) -> &'static str {
        match self {
            Module::SimpleSales => "Простые продажи",
            Module::ComplexSales => "Комплексные продажи",
            Module::Spendings => "Расходы",
            Module::Shifts => "Смена",
            Module::Stocks => "Остатки товаров",
            Module::Supplies => "Приемки",
            Module::Writeoffs => "Списания",
            Module::Revisions => "Ревизии",
            Module::Transfers => "Перемещения",
            Module::Salary => "Зарплата",
            Module::Checklist => "Чеклист",
            Module::Marketplace => "Онлайн каталог и заказы",
            Module::Updates => "Обновления",
            Module::Payments => "Платежи",
            Module::LabelPrinting => "Печать этикеток",
            Module::Manufacture => "Производство",
            Module::Clients => "Клиенты и Программа лояльности",
        }
    }
}

pub type ModuleFlags = BTreeMap<Module, bool>;

pub fn default_flags() -> ModuleFlags {
    Module::ALL.iter().map(|m| (*m, m.default_enabled())).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_reference_org() {
        let f = default_flags();
        assert_eq!(f.len(), 17);
        assert!(f[&Module::SimpleSales] && f[&Module::Clients]);
        assert!(!f[&Module::Shifts] && !f[&Module::Salary] && !f[&Module::Transfers]);
        assert_eq!(f.values().filter(|v| **v).count(), 8);
    }

    #[test]
    fn keys_match_serde_names() {
        for m in Module::ALL {
            let json = serde_json::to_string(&m).unwrap();
            assert_eq!(json, format!("\"{}\"", m.key()));
        }
    }
}
