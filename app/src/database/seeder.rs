use super::{Database, Transaction};
use crate::auth;
use chrono::{DateTime, Utc};
use uuid::Uuid;

pub async fn seed_development_data(db: &Database) {
    let mut data_tx = db.begin().await.unwrap();
    seed_test_user(&mut data_tx, 1).await;
    seed_test_user(&mut data_tx, 2).await;
    data_tx.commit().await.unwrap();
}

async fn seed_test_user(data_tx: &mut Transaction, index: u128) {
    let row = sqlx::query(r#"SELECT id FROM users WHERE id = $1"#)
        .bind(Uuid::from_u128(index))
        .fetch_optional(&mut *data_tx)
        .await
        .unwrap();
    if row.is_some() {
        return;
    }
    sqlx::query("INSERT INTO users (id, email, password, balance_msats, created) VALUES ($1, $2, $3, $4, $5)")
        .bind(Uuid::from_u128(index))
        .bind(format!("test-{}@user.net", index))
        .bind(format!("test-{}", index))
        .bind(2_000_000_000)
        .bind(Utc::now())
        .execute(&mut *data_tx)
        .await
        .unwrap();
    sqlx::query(
        r#"INSERT INTO auth_tokens (id, user_id, name, token_hash, can_spend, can_receive, can_read, created, disabled)
            VALUES($1, $2, $3, $4, $5, $6, $7, $8, $9)"#
    )
    .bind(Uuid::from_u128(index * 100 + 1))
    .bind(Uuid::from_u128(index))
    .bind(format!("spend_only_{}", index))
    .bind(auth::TokenHash::generate(&format!("spend_only_{}", index)).as_str())
    .bind(true)
    .bind(false)
    .bind(false)
    .bind(Utc::now())
    .bind(Option::<DateTime<Utc>>::None)
    .execute(&mut *data_tx)
    .await
    .unwrap();
    sqlx::query(
        r#"INSERT INTO auth_tokens (id, user_id, name, token_hash, can_spend, can_receive, can_read, created, disabled)
            VALUES($1, $2, $3, $4, $5, $6, $7, $8, $9)"#
    )
    .bind(Uuid::from_u128(index * 100 + 2))
    .bind(Uuid::from_u128(index))
    .bind(format!("receive_only_{}", index))
    .bind(auth::TokenHash::generate(&format!("receive_only_{}", index)).as_str())
    .bind(false)
    .bind(true)
    .bind(false)
    .bind(Utc::now())
    .bind(Option::<DateTime<Utc>>::None)
    .execute(&mut *data_tx)
    .await
    .unwrap();
    sqlx::query(
        r#"INSERT INTO auth_tokens (id, user_id, name, token_hash, can_spend, can_receive, can_read, created, disabled)
            VALUES($1, $2, $3, $4, $5, $6, $7, $8, $9)"#
    )
    .bind(Uuid::from_u128(index * 100 + 3))
    .bind(Uuid::from_u128(index))
    .bind(format!("read_only_{}", index))
    .bind(auth::TokenHash::generate(&format!("read_only_{}", index)).as_str())
    .bind(false)
    .bind(false)
    .bind(true)
    .bind(Utc::now())
    .bind(Option::<DateTime<Utc>>::None)
    .execute(&mut *data_tx)
    .await
    .unwrap();
    sqlx::query(
        r#"INSERT INTO auth_tokens (id, user_id, name, token_hash, can_spend, can_receive, can_read, created, disabled)
            VALUES($1, $2, $3, $4, $5, $6, $7, $8, $9)"#
    )
    .bind(Uuid::from_u128(index * 100 + 4))
    .bind(Uuid::from_u128(index))
    .bind(format!("all_{}", index))
    .bind(auth::TokenHash::generate(&format!("all_{}", index)).as_str())
    .bind(true)
    .bind(true)
    .bind(true)
    .bind(Utc::now())
    .bind(Option::<DateTime<Utc>>::None)
    .execute(&mut *data_tx)
    .await
    .unwrap();
    sqlx::query(
        r#"INSERT INTO auth_tokens (id, user_id, name, token_hash, can_spend, can_receive, can_read, created, disabled)
            VALUES($1, $2, $3, $4, $5, $6, $7, $8, $9)"#
    )
    .bind(Uuid::from_u128(index * 100 + 5))
    .bind(Uuid::from_u128(index))
    .bind(format!("disabled_{}", index))
    .bind(auth::TokenHash::generate(&format!("disabled_{}", index)).as_str())
    .bind(true)
    .bind(true)
    .bind(true)
    .bind(Utc::now())
    .bind(Some(Utc::now()))
    .execute(&mut *data_tx)
    .await
    .unwrap();
}
