use crate::{auth, database::Database};
use thiserror::Error;

mod entities;

pub use entities::{Email, Id, User};

pub async fn get(grant: &auth::ReadGrant, db: &Database) -> Option<User> {
    queries::get(db, grant.user_id).await
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("User being created already exists")]
    UserAlreadyExists,
}

mod queries {
    use super::{Email, Id, User};
    use crate::btc;
    use crate::database::Database;
    use chrono::{DateTime, Utc};
    use uuid::Uuid;

    pub(super) async fn get(db: &Database, id: Id) -> Option<User> {
        sqlx::query_as::<_, UserRow>(
            "SELECT id, email, balance_msats, created FROM users WHERE id = $1",
        )
        .bind(id.0)
        .fetch_optional(db)
        .await
        .unwrap()
        .map(|row| row.into_entity())
    }

    #[derive(sqlx::FromRow, Debug)]
    struct UserRow {
        id: Uuid,
        email: String,
        balance_msats: i64,
        created: DateTime<Utc>,
    }

    impl UserRow {
        fn into_entity(self) -> User {
            User {
                id: Id(self.id),
                email: Email(self.email),
                balance: btc::MilliSats(self.balance_msats),
                created: self.created,
            }
        }
    }
}
