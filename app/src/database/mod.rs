use sqlx::postgres::PgPoolOptions;
use url::Url;

pub use migrations::run_migrations;
pub use seeder::seed_development_data;

mod migrations;
mod seeder;

pub type Database = sqlx::Pool<sqlx::Postgres>;
pub(crate) type Transaction = sqlx::Transaction<'static, sqlx::Postgres>;

pub async fn connect(url: &Url) -> Database {
    PgPoolOptions::new().connect(url.as_str()).await.unwrap()
}

#[derive(Debug, sqlx::FromRow)]
pub(crate) struct CountRow {
    pub count: i64,
}

#[derive(Debug, sqlx::FromRow, Default)]
pub(crate) struct MaxRow<T> {
    pub max: Option<T>,
}

#[derive(Debug, sqlx::FromRow, Default)]
pub(crate) struct SumRow<T> {
    pub sum: T,
}
