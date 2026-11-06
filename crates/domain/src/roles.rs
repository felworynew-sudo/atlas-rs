//! Роли по умолчанию: «Админ» (всё) и «Продавец» (`cashier`).
use crate::permissions;

pub const ADMIN_KEY: &str = "admin";
pub const CASHIER_KEY: &str = "cashier";

pub struct DefaultRole {
    pub key: &'static str,
    pub name: &'static str,
    pub permissions: Vec<String>,
}

const CASHIER_PERMS: &[&str] = &[
    "sale.new",
    "sale.history",
    "sale.custom_product",
    "product.view",
    "stock.view",
    "spending.view",
    "revision.view",
    "writeoff.history",
    "client.view",
    "client.card",
    "shift.view",
    "checklist.view",
    "client_orders.view",
    "supply.history",
    "reports.view",
];

pub fn defaults() -> Vec<DefaultRole> {
    vec![
        DefaultRole { key: ADMIN_KEY, name: "Админ", permissions: permissions::all_keys() },
        DefaultRole {
            key: CASHIER_KEY,
            name: "Продавец",
            permissions: CASHIER_PERMS.iter().map(|s| s.to_string()).collect(),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cashier_perms_are_valid_and_admin_has_all() {
        for r in defaults() {
            for p in &r.permissions {
                assert!(permissions::is_known(p), "unknown permission {p}");
            }
        }
        assert_eq!(defaults()[0].permissions.len(), permissions::ALL.len());
    }
}
