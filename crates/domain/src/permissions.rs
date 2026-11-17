//! Права ролей (Настройки → Роли и доступы). key — стабильный идентификатор, title — подпись в UI.
use crate::modules::Module;

pub struct Permission {
    pub key: &'static str,
    pub title: &'static str,
    /// Модуль, без которого право не показывается (None — всегда).
    pub module: Option<Module>,
}

const fn p(key: &'static str, title: &'static str) -> Permission {
    Permission { key, title, module: None }
}

const fn pm(key: &'static str, title: &'static str, module: Module) -> Permission {
    Permission { key, title, module: Some(module) }
}

pub const ALL: &[Permission] = &[
    p("sale.new", "Новая продажа"),
    p("sale.apply_discount", "Применять скидки"),
    pm("sale.without_shift", "Пробивать продажи без открытия смены", Module::Shifts),
    p("sale.custom_product", "Продавать кастомные продукты"),
    p("sale.change_settings", "Менять настройки в Новой продаже"),
    p("sale.out_of_stock", "Продавать товары, которых нет в наличии"),
    p("sale.history", "История продаж"),
    p("sale.delete", "Удаление продажи"),
    pm("supply.history", "История приемок", Module::Supplies),
    pm("supply.delete", "Удаление приемки", Module::Supplies),
    pm("writeoff.history", "История списаний", Module::Writeoffs),
    pm("writeoff.delete", "Удаление списания", Module::Writeoffs),
    pm("spending.view", "Расходы", Module::Spendings),
    pm("spending.delete", "Удаление расхода", Module::Spendings),
    pm("spending.admin_categories", "Просмотр категорий расходов для админов", Module::Spendings),
    pm("revision.view", "Ревизии", Module::Revisions),
    pm("revision.delete", "Удаление ревизии", Module::Revisions),
    pm("manufacture.view", "Производство", Module::Manufacture),
    pm("manufacture.delete", "Удаление производства", Module::Manufacture),
    pm("transfer.view", "Перемещения товаров", Module::Transfers),
    pm("transfer.delete", "Удаление перемещения", Module::Transfers),
    pm("complex_sale.view", "Комплексные продажи", Module::ComplexSales),
    pm("complex_sale.delete", "Удаление комплексной продажи", Module::ComplexSales),
    pm("complex_sale.templates", "Редактирование шаблонов комплексных продаж", Module::ComplexSales),
    pm("payments.view", "Платежи", Module::Payments),
    p("product.view", "Товары"),
    p("product.delete", "Удаление товара"),
    p("product.inactive", "Просмотр неактивных товаров"),
    pm("stock.view", "Остатки товаров", Module::Stocks),
    pm("client.view", "Клиенты", Module::Clients),
    pm("client.card", "Карточка клиента", Module::Clients),
    pm("client.delete", "Удаление клиента", Module::Clients),
    pm("client.mailing", "Рассылки", Module::Clients),
    pm("labels.print", "Печать этикеток", Module::LabelPrinting),
    pm("updates.view", "Обновления", Module::Updates),
    pm("client_orders.view", "Заказы клиентов", Module::Marketplace),
    pm("checklist.view", "Чек-лист", Module::Checklist),
    pm("checklist.edit", "Редактирование чек-листа", Module::Checklist),
    pm("shift.view", "Смена", Module::Shifts),
    pm("shift.edit_staff", "Редактирование сотрудников в смене / рабочих часов других", Module::Shifts),
    pm("shift.edit_data", "Редактирование данных в смене", Module::Shifts),
    pm("shift.edit_old", "Правка закрытых смен старше недели", Module::Shifts),
    pm("salary.view", "Зарплата", Module::Salary),
    pm("salary.view_others", "Просмотр зарплат других сотрудников", Module::Salary),
    p("reports.home_stats", "Просмотр статистики продаж на главном экране"),
    p("reports.view", "Отчеты"),
    p("settings.categories", "Категории товаров"),
    p("settings.suppliers", "Поставщики"),
    p("settings.edit", "Редактирование настроек"),
    p("settings.users", "Редактирование пользователей"),
    p("settings.other_stores", "Просмотр других филиалов"),
    p("settings.switch_store", "Смена филиала"),
    p("settings.export_import", "Экспорт/Импорт"),
];

pub fn is_known(key: &str) -> bool {
    ALL.iter().any(|p| p.key == key)
}

/// Модуль, от которого зависит право (None — право доступно всегда).
pub fn module_of(key: &str) -> Option<Module> {
    ALL.iter().find(|p| p.key == key).and_then(|p| p.module)
}

pub fn all_keys() -> Vec<String> {
    ALL.iter().map(|p| p.key.to_string()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn keys_are_unique() {
        let set: HashSet<_> = ALL.iter().map(|p| p.key).collect();
        assert_eq!(set.len(), ALL.len());
    }
}
