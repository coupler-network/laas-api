use crate::{
    auth, balance, btc, chain, concurrency,
    database::Database,
    ln::{self, Lightning},
    swallow_panic, worker, QueryRange,
};
use async_trait::async_trait;
pub use entities::{Error, Id, Withdrawal};
use std::time::Duration;
use tokio::sync::Mutex;

mod entities;

pub async fn start(
    grant: &auth::SpendGrant,
    db: &Database,
    node: ln::Node,
    address: &btc::Address,
    amount: btc::Sats,
) -> Result<Withdrawal, Error> {
    let node = Mutex::new(node);
    concurrency::retry_loop(|| async {
        let mut data_tx = db.begin().await.unwrap();
        let mut balance = balance::get(&mut data_tx, grant.user_id).await;
        let mut node = node.lock().await;
        let (withdrawal, reservation) =
            Withdrawal::start(grant, &mut node, &mut balance, address.clone(), amount).await?;
        balance::update(&mut data_tx, &balance).await?;
        balance::upsert_reservation(&mut data_tx, &reservation).await;
        queries::upsert(&mut data_tx, &withdrawal).await;
        data_tx.commit().await.unwrap();
        Ok::<_, Error>(withdrawal)
    })
    .await
}

pub async fn get(grant: &auth::ReadGrant, db: &Database, id: Id) -> Option<Withdrawal> {
    queries::get(db, id, grant.user_id).await
}

pub async fn list(grant: &auth::ReadGrant, db: &Database, range: QueryRange) -> Vec<Withdrawal> {
    queries::list(db, grant.user_id, range).await
}

pub async fn start_workers(start_height: u32, db: &Database, lightning: &Lightning) {
    worker::start(WithdrawalSender {
        db: db.clone(),
        node: lightning.create_node().await,
    });
    chain::listen(start_height, db, lightning, Listener { db: db.clone() }).await;
}

struct WithdrawalSender {
    db: Database,
    node: ln::Node,
}

#[async_trait]
impl worker::Worker for WithdrawalSender {
    async fn run(&mut self) {
        let unsent_withdrawals = queries::list_unsent(&self.db).await;
        for mut withdrawal in unsent_withdrawals {
            swallow_panic(async {
                log::info!(
                    "sending withdrawal {:?} with amount {:?}",
                    withdrawal.id,
                    withdrawal.amount
                );
                let mut data_tx = self.db.begin().await.unwrap();
                // TODO Use PSBTs instead of this
                queries::lock(&mut data_tx, withdrawal.id).await;
                withdrawal.send(&mut self.node).await;
                queries::upsert(&mut data_tx, &withdrawal).await;
                data_tx.commit().await.unwrap();
            })
            .await;
        }
    }

    fn timeout() -> Duration {
        Duration::from_secs(10)
    }
}

struct Listener {
    db: Database,
}

#[async_trait]
impl chain::TxListener for Listener {
    async fn process(&mut self, tx_out: &btc::TxOut) {
        log::info!("processing transaction as withdrawal: {:?}", tx_out);
        if !tx_out.tx.is_confirmed() {
            log::info!("tx not confirmed: {:?}", tx_out);
            return;
        }
        match queries::get_by_tx_out(&self.db, &tx_out.tx.id, tx_out.v_out).await {
            Some(mut withdrawal) if !withdrawal.is_confirmed() => {
                log::info!("confirming withdrawal {:?}", withdrawal.id);
                let mut data_tx = self.db.begin().await.unwrap();
                let mut reservation =
                    balance::get_reservation(&self.db, withdrawal.reservation_id).await;
                withdrawal.confirm(tx_out, &mut reservation);
                queries::upsert(&mut data_tx, &withdrawal).await;
                balance::upsert_reservation(&mut data_tx, &reservation).await;
                data_tx.commit().await.unwrap();
            }
            Some(withdrawal) => log::info!(
                "withdrawal {:?} already confirmed by txout {:?}",
                withdrawal.id,
                tx_out
            ),
            None => log::info!("no withdrawals confirmed by txout {:?}", tx_out),
        }
    }
}

mod queries {
    use super::{Id, Withdrawal};
    use crate::{
        auth, balance, btc,
        database::{self, Database},
        user, QueryRange,
    };
    use chrono::{DateTime, Utc};
    use std::str::FromStr;
    use uuid::Uuid;

    pub(super) async fn get_by_tx_out(
        db: &Database,
        tx_id: &btc::TxId,
        v_out: i64,
    ) -> Option<Withdrawal> {
        sqlx::query_as::<_, WithdrawalRow>(
            r#"SELECT
                withdrawals.id,
                withdrawals.user_id,
                withdrawals.token_id,
                withdrawals.reservation_id,
                withdrawals.address,
                withdrawals.fee_sats,
                withdrawals.amount_sats,
                withdrawals.tx_id,
                withdrawals.v_out,
                withdrawals.created,
                withdrawals.confirmed,
                tx_outs.block_height
            FROM withdrawals
            JOIN tx_outs ON withdrawals.tx_id = tx_outs.tx_id AND withdrawals.v_out = tx_outs.v_out
            WHERE withdrawals.tx_id = $1 AND withdrawals.v_out = $2"#,
        )
        .bind(tx_id.to_string())
        .bind(v_out)
        .fetch_optional(db)
        .await
        .unwrap()
        .map(|row| row.into_entity())
    }

    pub(super) async fn list_unsent(db: &Database) -> Vec<Withdrawal> {
        sqlx::query_as::<_, WithdrawalRow>(
            r#"SELECT id, user_id, token_id, reservation_id, address, fee_sats, amount_sats, tx_id, v_out, created, confirmed, NULL AS block_height
                FROM withdrawals WHERE tx_id IS NULL"#,
        )
        .fetch_all(db)
        .await
        .unwrap()
        .into_iter()
        .map(|row| row.into_entity())
        .collect()
    }

    pub(super) async fn lock(data_tx: &mut database::Transaction, id: Id) {
        sqlx::query("SELECT id FROM withdrawals WHERE id = $1 FOR UPDATE")
            .bind(id.0)
            .fetch_one(data_tx)
            .await
            .unwrap();
    }

    pub(super) async fn upsert(data_tx: &mut database::Transaction, withdrawal: &Withdrawal) {
        if let Some(tx_out) = withdrawal.tx_out.as_ref() {
            sqlx::query(
                r#"INSERT INTO tx_outs (tx_id, block_height, address, v_out, amount_sats)
                    VALUES ($1, $2, $3, $4, $5) ON CONFLICT (tx_id, v_out) DO UPDATE SET
                    block_height = $2, address = $3, amount_sats = $5"#,
            )
            .bind(tx_out.tx.id.to_string())
            .bind(tx_out.tx.block_height)
            .bind(tx_out.address.to_string())
            .bind(tx_out.v_out)
            .bind(tx_out.amount.0)
            .execute(&mut *data_tx)
            .await
            .unwrap();
        }
        sqlx::query(
            r#"INSERT INTO withdrawals (id, user_id, token_id, reservation_id, address, fee_sats, amount_sats, tx_id, v_out, created, confirmed)
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11) ON CONFLICT (id) DO UPDATE SET
                user_id = $2, token_id = $3, reservation_id = $4, address = $5, fee_sats = $6, amount_sats = $7, tx_id = $8, v_out = $9, created = $10, confirmed = $11"#,
        )
        .bind(withdrawal.id.0)
        .bind(withdrawal.user_id.0)
        .bind(withdrawal.token_id.0)
        .bind(withdrawal.reservation_id.0)
        .bind(withdrawal.address.to_string())
        .bind(withdrawal.fee.0)
        .bind(withdrawal.amount.0)
        .bind(withdrawal.tx_out.as_ref().map(|tx_out| tx_out.tx.id.to_string()))
        .bind(withdrawal.tx_out.as_ref().map(|tx_out| tx_out.v_out))
        .bind(withdrawal.created)
        .bind(withdrawal.confirmed)
        .execute(&mut *data_tx)
        .await
        .unwrap();
    }

    pub(super) async fn get(db: &Database, id: Id, user_id: user::Id) -> Option<Withdrawal> {
        sqlx::query_as::<_, WithdrawalRow>(
            r#"SELECT id, user_id, token_id, reservation_id, address, fee_sats, amount_sats, tx_id, v_out, created, confirmed, NULL AS block_height
                FROM withdrawals WHERE id = $1 AND user_id = $2"#,
        )
        .bind(id.0)
        .bind(user_id.0)
        .fetch_optional(db)
        .await
        .unwrap()
        .map(|row|row.into_entity())
    }

    pub(super) async fn list(
        db: &Database,
        user_id: user::Id,
        range: QueryRange,
    ) -> Vec<Withdrawal> {
        sqlx::query_as::<_, WithdrawalRow>(
            r#"SELECT id, user_id, token_id, reservation_id, address, fee_sats, amount_sats, tx_id, v_out, created, confirmed, NULL AS block_height
                FROM withdrawals WHERE user_id = $1 ORDER BY created DESC LIMIT $2 OFFSET $3"#,
        )
        .bind(user_id.0)
        .bind(range.limit)
        .bind(range.offset)
        .fetch_all(db)
        .await
        .unwrap()
        .into_iter()
        .map(|row|row.into_entity())
        .collect()
    }

    #[derive(sqlx::FromRow, Debug)]
    struct WithdrawalRow {
        id: Uuid,
        token_id: Uuid,
        user_id: Uuid,
        reservation_id: Uuid,
        address: String,
        fee_sats: i64,
        amount_sats: i64,
        tx_id: Option<String>,
        v_out: Option<i32>,
        block_height: Option<i32>,
        created: DateTime<Utc>,
        confirmed: Option<DateTime<Utc>>,
    }

    impl WithdrawalRow {
        fn into_entity(self) -> Withdrawal {
            Withdrawal {
                id: Id(self.id),
                token_id: auth::TokenId(self.token_id),
                user_id: user::Id(self.user_id),
                reservation_id: balance::ReservationId(self.reservation_id),
                address: btc::Address::from_str(&self.address).unwrap(),
                fee: btc::Sats(self.fee_sats),
                amount: btc::Sats(self.amount_sats),
                tx_out: match (self.tx_id, self.v_out) {
                    (Some(tx_id), Some(v_out)) => Some(btc::TxOut {
                        tx: btc::Tx {
                            id: btc::TxId::from_str(&tx_id).unwrap(),
                            block_height: self.block_height.map(|x| x.try_into().unwrap()),
                        },
                        address: btc::Address::from_str(&self.address).unwrap(),
                        v_out: v_out.try_into().unwrap(),
                        amount: btc::Sats(self.amount_sats),
                    }),
                    _ => None,
                },
                created: self.created,
                confirmed: self.confirmed,
            }
        }
    }
}
