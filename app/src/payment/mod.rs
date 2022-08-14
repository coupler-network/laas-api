use crate::{
    auth, balance, btc, cash_limits::CashLimits, concurrency, database::Database, ln, QueryRange,
};
use tokio::sync::Mutex;

mod entities;

pub use entities::{Error, Id, Payment, Status};

pub async fn send(
    grant: &auth::SpendGrant,
    db: &Database,
    node: ln::Node,
    invoice: ln::RawInvoice,
    amount: Option<btc::MilliSats>,
    limits: &CashLimits,
) -> Result<Payment, Error> {
    let daily_total = queries::daily_total(db, grant.user_id).await;
    let payment = Payment::create(grant, invoice, amount, limits, daily_total)?;

    let mut data_tx = db.begin().await.unwrap();
    queries::upsert(&mut data_tx, &payment).await;
    data_tx.commit().await.unwrap();

    let payment = Mutex::new(payment);
    let node = Mutex::new(node);
    concurrency::retry_loop(|| async {
        let mut data_tx = db.begin().await.unwrap();
        let mut balance = balance::get(&mut data_tx, grant.user_id).await;
        let mut payment = payment.lock().await;
        let mut node = node.lock().await;

        let result = payment.prepare(&mut node, &mut balance).await;

        if let Ok(ref reservation) = result {
            balance::upsert_reservation(&mut data_tx, reservation).await;
        }
        queries::upsert(&mut data_tx, &payment).await;
        balance::update(&mut data_tx, &balance).await?;
        data_tx.commit().await.unwrap();
        result
    })
    .await?;

    concurrency::retry_loop(|| async {
        let mut data_tx = db.begin().await.unwrap();
        let mut balance = balance::get(&mut data_tx, grant.user_id).await;
        let mut payment = payment.lock().await;
        let mut node = node.lock().await;
        let mut reservation = balance::get_reservation(db, payment.reservation_id.unwrap()).await;

        let result = payment
            .send(&mut node, &mut balance, &mut reservation)
            .await;

        balance::upsert_reservation(&mut data_tx, &reservation).await;
        queries::upsert(&mut data_tx, &payment).await;
        balance::update(&mut data_tx, &balance).await?;
        data_tx.commit().await.unwrap();
        result
    })
    .await?;

    Ok(payment.into_inner())
}

pub async fn get(grant: &auth::ReadGrant, db: &Database, id: Id) -> Option<Payment> {
    queries::get(db, id, grant.user_id).await
}

pub async fn list(grant: &auth::ReadGrant, db: &Database, range: QueryRange) -> Vec<Payment> {
    queries::list(db, grant.user_id, range).await
}

mod queries {
    use super::{Id, Payment, Status};
    use crate::{
        auth, balance, btc,
        database::{self, Database, SumRow},
        ln, user, QueryRange,
    };
    use chrono::{DateTime, Duration, Utc};
    use const_format::formatcp;
    use uuid::Uuid;

    const COLUMNS: &str = "id, user_id, token_id, reservation_id, amount_msats, fee_msats, invoice, created, status, failure_reason, failure_timestamp, success_timestamp";

    pub(super) async fn upsert(data_tx: &mut database::Transaction, payment: &Payment) {
        sqlx::query(
            formatcp!(
            r#"INSERT INTO payments ({})
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12) ON CONFLICT (id) DO UPDATE SET
                user_id = $2, token_id = $3, reservation_id = $4, amount_msats = $5, fee_msats = $6, invoice = $7, created = $8, status = $9, failure_reason = $10, failure_timestamp = $11, success_timestamp = $12"#,
                COLUMNS)
        )
        .bind(payment.id.0)
        .bind(payment.user_id.0)
        .bind(payment.token_id.0)
        .bind(payment.reservation_id.map(|id| id.0))
        .bind(payment.amount.0)
        .bind(payment.fee.map(|fee| fee.0))
        .bind(&payment.invoice.0)
        .bind(payment.created)
        .bind(status_to_i32(&payment.status))
        .bind(match payment.status {
            Status::Failed{ ref reason, timestamp: _ } => Some(reason.clone()),
            _ => None
        })
        .bind(match payment.status {
            Status::Failed{ reason: _, timestamp } => Some(timestamp),
            _ => None
        })
        .bind(match payment.status {
            Status::Succeeded{ timestamp } => Some(timestamp),
            _ => None
        })
        .execute(&mut *data_tx)
        .await
        .unwrap();
    }

    pub(super) async fn get(db: &Database, id: Id, user_id: user::Id) -> Option<Payment> {
        sqlx::query_as::<_, PaymentRow>(formatcp!(
            "SELECT {} FROM payments WHERE id = $1 AND user_id = $2",
            COLUMNS
        ))
        .bind(id.0)
        .bind(user_id.0)
        .fetch_optional(db)
        .await
        .unwrap()
        .map(|row| row.into_entity())
    }

    pub(super) async fn list(db: &Database, user_id: user::Id, range: QueryRange) -> Vec<Payment> {
        sqlx::query_as::<_, PaymentRow>(formatcp!(
            "SELECT {} FROM payments WHERE user_id = $1 ORDER BY created DESC LIMIT $2 OFFSET $3",
            COLUMNS
        ))
        .bind(user_id.0)
        .bind(range.limit)
        .bind(range.offset)
        .fetch_all(db)
        .await
        .unwrap()
        .into_iter()
        .map(|row| row.into_entity())
        .collect()
    }

    pub(super) async fn daily_total(db: &Database, user_id: user::Id) -> btc::MilliSats {
        sqlx::query_as::<_, SumRow<Option<i64>>>(
            "SELECT SUM(CAST(amount_msats AS INTEGER)) AS sum FROM payments WHERE user_id = $1 AND created > $2",
        )
        .bind(user_id.0)
        .bind(Utc::now() - Duration::days(1))
        .fetch_one(db)
        .await
        .unwrap()
        .sum
        .map(btc::MilliSats)
        .unwrap_or_default()
    }

    #[derive(sqlx::FromRow, Debug)]
    struct PaymentRow {
        id: Uuid,
        token_id: Uuid,
        user_id: Uuid,
        reservation_id: Option<Uuid>,
        amount_msats: i64,
        fee_msats: Option<i64>,
        invoice: String,
        created: DateTime<Utc>,
        status: i32,
        failure_reason: Option<String>,
        failure_timestamp: Option<DateTime<Utc>>,
        success_timestamp: Option<DateTime<Utc>>,
    }

    impl PaymentRow {
        fn into_entity(self) -> Payment {
            let status = self.status();
            Payment {
                id: Id(self.id),
                token_id: auth::TokenId(self.token_id),
                user_id: user::Id(self.user_id),
                amount: btc::MilliSats(self.amount_msats),
                fee: self.fee_msats.map(btc::MilliSats),
                invoice: ln::RawInvoice(self.invoice),
                reservation_id: self.reservation_id.map(balance::ReservationId),
                created: self.created,
                status,
            }
        }

        fn status(&self) -> Status {
            match self.status {
                0 => Status::New,
                1 => Status::Ready,
                2 => Status::Succeeded {
                    timestamp: self.success_timestamp.unwrap(),
                },
                3 => Status::Failed {
                    reason: self.failure_reason.as_ref().cloned().unwrap(),
                    timestamp: self.failure_timestamp.unwrap(),
                },
                _ => unreachable!("invalid status {:?}", self.status),
            }
        }
    }

    fn status_to_i32(status: &Status) -> i32 {
        match status {
            Status::New => 0,
            Status::Ready => 0,
            Status::Succeeded { .. } => 2,
            Status::Failed { .. } => 3,
        }
    }
}
