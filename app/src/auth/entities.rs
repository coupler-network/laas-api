//! Handles user authentication, authorization, and tokens. Authentication is proven by possession
//! of a token; authorization is proven by possession of a grant. There are two different grants:
//! spend and receive, and they're encoded as two separate types in the type system.

use crate::{hex::Hex, user};
use chrono::{DateTime, Utc};
use sha2::Digest;
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error)]
#[error("access denied")]
pub struct AccessDenied;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct TokenId(pub Uuid);

/// This grant represents a compile-time proof that the token is authorized to spend funds.
#[derive(Debug)]
pub struct SpendGrant {
    pub token_id: TokenId,
    pub user_id: user::Id,
}

/// This grant represents a compile-time proof that the token is authorized to receive funds.
#[derive(Debug)]
pub struct ReceiveGrant {
    pub token_id: TokenId,
    pub user_id: user::Id,
}

/// This grant represents a compile-time proof that the token is authorized to read data.
#[derive(Debug)]
pub struct ReadGrant {
    pub token_id: TokenId,
    pub user_id: user::Id,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Permissions {
    pub can_spend: bool,
    pub can_receive: bool,
    pub can_read: bool,
}

/// A hash of the token.
pub struct TokenHash(Hex);

impl TokenHash {
    /// Hashes a token with a specific hashing algorithm.
    ///
    /// Currently, SHA256 is used, without salting. The reason why a fast algorithm like SHA256 is
    /// good enough is because the tokens are generated randomly, so they have a high entropy. High
    /// entropy is also the reason why salting is unnecessary.
    pub(crate) fn generate(token: &str) -> Self {
        let mut hasher = sha2::Sha256::new();
        hasher.update(token);
        let sha = hasher.finalize();
        Self(Hex::encode(&sha))
    }

    pub(crate) fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

/// A token proves the identity of a user. A user can generate as many tokens as he wants, with
/// different or same permissions.
#[derive(Debug)]
pub struct Token {
    pub(crate) id: TokenId,
    pub(crate) user_id: user::Id,
    pub(crate) permissions: Permissions,
    pub(crate) disabled: Option<DateTime<Utc>>,
}

impl Token {
    pub(crate) fn spend_grant(&self) -> Result<SpendGrant, AccessDenied> {
        if self.is_enabled() && self.permissions.can_spend {
            Ok(SpendGrant {
                token_id: self.id,
                user_id: self.user_id,
            })
        } else {
            Err(AccessDenied)
        }
    }

    pub(crate) fn receive_grant(&self) -> Result<ReceiveGrant, AccessDenied> {
        if self.is_enabled() && self.permissions.can_receive {
            Ok(ReceiveGrant {
                token_id: self.id,
                user_id: self.user_id,
            })
        } else {
            Err(AccessDenied)
        }
    }

    pub(crate) fn read_grant(&self) -> Result<ReadGrant, AccessDenied> {
        if self.is_enabled() && self.permissions.can_read {
            Ok(ReadGrant {
                token_id: self.id,
                user_id: self.user_id,
            })
        } else {
            Err(AccessDenied)
        }
    }

    fn is_enabled(&self) -> bool {
        self.disabled.is_none()
    }
}
