use crate::database::Database;

mod entities;

pub use entities::{AccessDenied, ReadGrant, ReceiveGrant, SpendGrant, TokenHash, TokenId};

pub async fn get_spend_grant(db: &Database, token: &str) -> Result<SpendGrant, AccessDenied> {
    queries::get_token(db, token)
        .await
        .ok_or(AccessDenied)?
        .spend_grant()
}

pub async fn get_receive_grant(db: &Database, token: &str) -> Result<ReceiveGrant, AccessDenied> {
    queries::get_token(db, token)
        .await
        .ok_or(AccessDenied)?
        .receive_grant()
}

pub async fn get_read_grant(db: &Database, token: &str) -> Result<ReadGrant, AccessDenied> {
    queries::get_token(db, token)
        .await
        .ok_or(AccessDenied)?
        .read_grant()
}

mod queries {
    use super::entities::{Permissions, Token};
    use super::{TokenHash, TokenId};
    use crate::{database::Database, user};
    use chrono::{DateTime, Utc};
    use uuid::Uuid;

    pub(super) async fn get_token(db: &Database, token: &str) -> Option<Token> {
        let token_hash = TokenHash::generate(token);
        sqlx::query_as::<_, TokenRow>(
            r#"SELECT id, user_id, can_spend, can_receive, can_read, disabled FROM auth_tokens
                WHERE token_hash = $1"#,
        )
        .bind(token_hash.as_str())
        .fetch_optional(db)
        .await
        .unwrap()
        .map(|row| row.into_entity())
    }

    #[derive(Debug, sqlx::FromRow)]
    struct TokenRow {
        id: Uuid,
        user_id: Uuid,
        can_spend: bool,
        can_receive: bool,
        can_read: bool,
        disabled: Option<DateTime<Utc>>,
    }

    impl TokenRow {
        fn into_entity(self) -> Token {
            Token {
                id: TokenId(self.id),
                user_id: user::Id(self.user_id),
                permissions: Permissions {
                    can_spend: self.can_spend,
                    can_receive: self.can_receive,
                    can_read: self.can_read,
                },
                disabled: self.disabled,
            }
        }
    }
}
