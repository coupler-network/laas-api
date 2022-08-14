use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::btc;

#[derive(Debug)]
pub struct Email(pub String);

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Id(pub Uuid);

#[derive(Debug)]
pub struct User {
    pub id: Id,
    pub email: Email,
    pub balance: btc::MilliSats,
    pub created: DateTime<Utc>,
}
