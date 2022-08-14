use crate::btc;
use crate::concurrency;
use crate::database;
use crate::user;
use chrono::DateTime;
use chrono::Utc;
use uuid::Uuid;

mod entities;

pub use entities::{Balance, InsufficientBalance, Reservation, ReservationId, ReservationStatus};

pub async fn get(data_tx: &mut database::Transaction, user_id: user::Id) -> Balance {
    sqlx::query_as::<_, BalanceRow>("SELECT id AS user_id, balance_msats FROM users WHERE id = $1")
        .bind(user_id.0)
        .fetch_one(data_tx)
        .await
        .unwrap()
        .into_entity()
}

pub async fn update(
    data_tx: &mut database::Transaction,
    balance: &Balance,
) -> Result<(), concurrency::ConflictError> {
    if balance.changed() {
        sqlx::query(
            "UPDATE users SET balance_msats = $1 WHERE id = $2 AND balance_msats = $3 RETURNING id",
        )
        .bind(balance.amount().0)
        .bind(balance.user_id().0)
        .bind(balance.original_amount().0)
        .fetch_optional(data_tx)
        .await
        .unwrap()
        .ok_or(concurrency::ConflictError)?;
    }
    Ok(())
}

pub async fn upsert_reservation(data_tx: &mut database::Transaction, reservation: &Reservation) {
    sqlx::query(
        r#"INSERT INTO balance_reservations (id, user_id, amount_msats, status, created)
            VALUES ($1, $2, $3, $4, $5) ON CONFLICT (id) DO UPDATE SET
            user_id = $2, amount_msats = $3, status = $4, created = $5 WHERE balance_reservations.status = 0
            RETURNING id"#,
    )
    .bind(reservation.id.0)
    .bind(reservation.user_id.0)
    .bind(reservation.amount.0)
    .bind(match reservation.status {
        ReservationStatus::Pending => 0,
        ReservationStatus::Debited => 1,
        ReservationStatus::Refunded => 2,
    })
    .bind(reservation.created)
    .fetch_one(data_tx)
    .await
    .unwrap();
}

pub async fn get_reservation(db: &database::Database, id: ReservationId) -> Reservation {
    sqlx::query_as::<_, ReservationRow>(
        "SELECT id, user_id, amount_msats, status, created FROM balance_reservations WHERE id = $1",
    )
    .bind(id.0)
    .fetch_one(db)
    .await
    .unwrap()
    .into_entity()
}

#[derive(sqlx::FromRow, Debug)]
struct BalanceRow {
    user_id: Uuid,
    balance_msats: i64,
}

impl BalanceRow {
    fn into_entity(self) -> Balance {
        Balance::new(user::Id(self.user_id), btc::MilliSats(self.balance_msats))
    }
}

#[derive(sqlx::FromRow, Debug)]
struct ReservationRow {
    id: Uuid,
    user_id: Uuid,
    amount_msats: i64,
    status: i32,
    created: DateTime<Utc>,
}

impl ReservationRow {
    fn into_entity(self) -> Reservation {
        Reservation {
            id: ReservationId(self.id),
            user_id: user::Id(self.user_id),
            amount: btc::MilliSats(self.amount_msats),
            status: match self.status {
                0 => ReservationStatus::Pending,
                1 => ReservationStatus::Debited,
                2 => ReservationStatus::Refunded,
                _ => unreachable!("unknown status number"),
            },
            created: self.created,
        }
    }
}
