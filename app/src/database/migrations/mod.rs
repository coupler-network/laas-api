//! This module is in charge of migrations.
//! Add migrations as submodules to this module.

use super::{CountRow, Database};
use async_trait::async_trait;
use sqlx::Transaction;
use std::borrow::BorrowMut;

mod m0000_init;

#[async_trait]
pub trait Migration {
    fn serial_number(&self) -> i64;
    async fn run(&self, tx: &mut Transaction<sqlx::Postgres>);
}

struct SimpleSqlMigration {
    pub serial_number: i64,
    pub sql: Vec<&'static str>,
}

#[async_trait]
impl Migration for SimpleSqlMigration {
    fn serial_number(&self) -> i64 {
        self.serial_number
    }

    async fn run(&self, tx: &mut Transaction<sqlx::Postgres>) {
        for sql in self.sql.iter() {
            sqlx::query(sql).execute(tx.borrow_mut()).await.unwrap();
        }
    }
}

// TODO Implement some sort of sanity check that serial numbers are not used multiple times

/// Execute all migrations on the database.
pub async fn run_migrations(db: &Database) {
    prepare_migrations_table(db).await;
    run_migration(m0000_init::migration(), db).await;
}

async fn prepare_migrations_table(db: &Database) {
    sqlx::query("CREATE TABLE IF NOT EXISTS migrations (serial_number bigint)")
        .execute(db)
        .await
        .unwrap();
}

async fn run_migration(migration: impl Migration, db: &Database) {
    let row = sqlx::query_as::<_, CountRow>(
        "SELECT COUNT(*) AS count FROM migrations WHERE serial_number = $1",
    )
    .bind(migration.serial_number())
    .fetch_one(db)
    .await
    .unwrap();

    if row.count > 0 {
        return;
    }

    let mut transaction = db.begin().await.unwrap();
    migration.run(&mut transaction).await;

    sqlx::query("INSERT INTO migrations VALUES ($1)")
        .bind(migration.serial_number())
        .execute(&mut transaction)
        .await
        .unwrap();

    transaction.commit().await.unwrap();
}
