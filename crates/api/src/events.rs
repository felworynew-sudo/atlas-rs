//! Лента «Обновления»: запись событий организации (цены, удаления документов).
use sqlx::PgPool;
use uuid::Uuid;

/// Записывает событие; ошибка записи ленты не должна ломать основную операцию.
pub async fn log(pool: &PgPool, org_id: Uuid, store_id: Option<Uuid>, user_id: Uuid, kind: &str, text: &str) {
    let res = sqlx::query("INSERT INTO update_events (id, organization_id, store_id, user_id, kind, text) VALUES ($1,$2,$3,$4,$5,$6)")
        .bind(Uuid::new_v4())
        .bind(org_id)
        .bind(store_id)
        .bind(user_id)
        .bind(kind)
        .bind(text)
        .execute(pool)
        .await;
    if let Err(e) = res {
        tracing::warn!(error = %e, kind, "не удалось записать событие");
    }
}

/// «1 200» — число без хвостовых нулей для текста событий.
pub fn fmt(v: f64) -> String {
    let s = format!("{v:.2}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

#[cfg(test)]
mod tests {
    use super::fmt;

    #[test]
    fn formats_numbers() {
        assert_eq!(fmt(1200.0), "1200");
        assert_eq!(fmt(99.5), "99.5");
        assert_eq!(fmt(0.0), "0");
    }
}
