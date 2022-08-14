use crate::auth;
use crate::balance;
use crate::btc;
use crate::chain;
use crate::concurrency;
use crate::database::{self, Database};
use crate::ln;
use crate::QueryRange;
use async_trait::async_trait;

mod entities;

pub use entities::{Address, Deposit, Id};

pub async fn create_address(
    grant: &auth::ReceiveGrant,
    db: &Database,
    mut node: ln::Node,
) -> Address {
    let mut transaction = db.begin().await.unwrap();
    let address = Address::generate(grant, &mut node).await;
    queries::insert_address(&mut transaction, &address).await;
    transaction.commit().await.unwrap();
    address
}

pub async fn get_address(
    grant: &auth::ReadGrant,
    db: &Database,
    address: &btc::Address,
) -> Option<Address> {
    queries::get_address_for_user(db, address, grant.user_id).await
}

pub async fn get_addresses(
    grant: &auth::ReadGrant,
    db: &Database,
    range: QueryRange,
) -> Vec<Address> {
    queries::get_addresses_for_user(db, range, grant.user_id).await
}

pub async fn get(grant: &auth::ReadGrant, db: &Database, id: Id) -> Option<Deposit> {
    queries::get_for_user(db, id, grant.user_id).await
}

pub async fn list(grant: &auth::ReadGrant, db: &Database, range: QueryRange) -> Vec<Deposit> {
    queries::list_for_user(db, grant.user_id, range).await
}

pub async fn start_worker(start_height: u32, db: &Database, lightning: &ln::Lightning) {
    chain::listen(start_height, db, lightning, Listener { db: db.clone() }).await;
}

struct Listener {
    db: Database,
}

#[async_trait]
impl chain::TxListener for Listener {
    async fn process(&mut self, tx_out: &btc::TxOut) {
        concurrency::retry_loop(|| async {
            log::info!("processing transaction as deposit: {:?}", tx_out);
            let mut data_tx = self.db.begin().await.unwrap();
            match get_or_start(&mut data_tx, tx_out).await? {
                Some(mut deposit) => {
                    if tx_out.tx.is_confirmed() && !deposit.is_confirmed() {
                        log::info!("confirming deposit {:?}", deposit.id);
                        let mut balance = balance::get(&mut data_tx, deposit.user_id).await;
                        deposit.confirm(tx_out, &mut balance);
                        queries::upsert(&mut data_tx, &deposit).await?;
                        balance::update(&mut data_tx, &balance).await?;
                    } else {
                        log::info!("not confirming deposit {:?}", deposit.id);
                    }
                    data_tx.commit().await.unwrap();
                }
                None => log::info!("txout {:?} not related to a deposit", tx_out),
            };
            Ok::<_, concurrency::ConflictError>(())
        })
        .await
        .unwrap();
    }
}

async fn get_or_start(
    data_tx: &mut database::Transaction,
    tx_out: &btc::TxOut,
) -> Result<Option<Deposit>, concurrency::ConflictError> {
    match queries::get(data_tx, &tx_out.tx.id, tx_out.v_out).await {
        Some(deposit) => Ok(Some(deposit)),
        None => Ok(start(data_tx, tx_out).await?),
    }
}

async fn start(
    data_tx: &mut database::Transaction,
    tx_out: &btc::TxOut,
) -> Result<Option<Deposit>, concurrency::ConflictError> {
    match queries::get_address(data_tx, &tx_out.address).await {
        Some(deposit_address) => {
            log::info!("starting deposit for {:?}", deposit_address);
            let deposit = deposit_address.start_deposit(tx_out).await;
            queries::upsert(data_tx, &deposit).await?;
            Ok(Some(deposit))
        }
        None => Ok(None),
    }
}

mod queries {
    use super::{Address, Deposit, Id};
    use crate::auth;
    use crate::btc;
    use crate::concurrency;
    use crate::database;
    use crate::database::Database;
    use crate::user;
    use crate::QueryRange;
    use chrono::{DateTime, Utc};
    use std::str::FromStr;
    use uuid::Uuid;

    pub(super) async fn insert_address(data_tx: &mut database::Transaction, address: &Address) {
        sqlx::query(
            "INSERT INTO deposit_addresses (user_id, token_id, address, created) VALUES ($1, $2, $3, $4)",
        )
        .bind(address.user_id.0)
        .bind(address.token_id.0)
        .bind(address.address.to_string())
        .bind(address.created)
        .execute(data_tx)
        .await
        .unwrap();
    }

    pub(super) async fn get_address(
        data_tx: &mut database::Transaction,
        address: &btc::Address,
    ) -> Option<Address> {
        sqlx::query_as::<_, DepositAddressRow>(
            "SELECT user_id, token_id, address, created FROM deposit_addresses WHERE address = $1",
        )
        .bind(address.to_string())
        .fetch_optional(data_tx)
        .await
        .unwrap()
        .map(|row| row.into_entity())
    }

    pub(super) async fn get_address_for_user(
        db: &Database,
        address: &btc::Address,
        user_id: user::Id,
    ) -> Option<Address> {
        sqlx::query_as::<_, DepositAddressRow>(
            "SELECT user_id, token_id, address, created FROM deposit_addresses WHERE address = $1 AND user_id = $2",
        )
        .bind(address.to_string())
        .bind(user_id.0)
        .fetch_optional(db)
        .await
        .unwrap()
        .map(|row| row.into_entity())
    }

    pub(super) async fn get_addresses_for_user(
        db: &Database,
        range: QueryRange,
        user_id: user::Id,
    ) -> Vec<Address> {
        sqlx::query_as::<_, DepositAddressRow>(
            r#"SELECT user_id, token_id, address, created FROM deposit_addresses
                WHERE user_id = $1 ORDER BY created DESC LIMIT $2 OFFSET $3"#,
        )
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

    pub(super) async fn upsert(
        data_tx: &mut database::Transaction,
        deposit: &Deposit,
    ) -> Result<(), concurrency::ConflictError> {
        sqlx::query(
            r#"INSERT INTO tx_outs (tx_id, block_height, address, v_out, amount_sats)
                VALUES ($1, $2, $3, $4, $5) ON CONFLICT (tx_id, v_out) DO UPDATE SET
                block_height = $2, address = $3, amount_sats = $5"#,
        )
        .bind(deposit.tx_out.tx.id.to_string())
        .bind(deposit.tx_out.tx.block_height)
        .bind(deposit.tx_out.address.to_string())
        .bind(deposit.tx_out.v_out)
        .bind(deposit.tx_out.amount.0)
        .execute(&mut *data_tx)
        .await
        .unwrap();
        match sqlx::query(
            r#"INSERT INTO deposits (id, user_id, tx_id, v_out, address, created, confirmed)
                VALUES ($1, $2, $3, $4, $5, $6, $7) ON CONFLICT (id) DO UPDATE SET
                user_id = $2, tx_id = $3, v_out = $4, address = $5, created = $6, confirmed = $7"#,
        )
        .bind(deposit.id.0)
        .bind(deposit.user_id.0)
        .bind(deposit.tx_out.tx.id.to_string())
        .bind(deposit.tx_out.v_out)
        .bind(deposit.tx_out.address.to_string())
        .bind(deposit.created)
        .bind(deposit.confirmed)
        .execute(&mut *data_tx)
        .await
        {
            Ok(_) => Ok(()),
            Err(e)
                if e.to_string().to_lowercase().contains(
                    "duplicate key value violates unique constraint \"deposit_tx_id_v_out\"",
                ) =>
            {
                Err(concurrency::ConflictError)
            }
            Err(e) => panic!("{:?}", e),
        }
    }

    pub(super) async fn get(
        data_tx: &mut database::Transaction,
        tx_id: &btc::TxId,
        v_out: i64,
    ) -> Option<Deposit> {
        sqlx::query_as::<_, DepositRow>(
            r#"SELECT
                deposits.id,
                deposits.user_id,
                deposits.tx_id,
                deposits.v_out,
                deposits.created,
                deposits.confirmed,
                tx_outs.block_height,
                tx_outs.address,
                tx_outs.amount_sats
            FROM deposits
            JOIN tx_outs ON deposits.tx_id = tx_outs.tx_id AND deposits.v_out = tx_outs.v_out
            WHERE deposits.tx_id = $1 AND deposits.v_out = $2"#,
        )
        .bind(tx_id.to_string())
        .bind(v_out)
        .fetch_optional(data_tx)
        .await
        .unwrap()
        .map(|row| row.into_entity())
    }

    pub(super) async fn get_for_user(db: &Database, id: Id, user_id: user::Id) -> Option<Deposit> {
        sqlx::query_as::<_, DepositRow>(
            r#"SELECT
                deposits.id,
                deposits.user_id,
                deposits.tx_id,
                deposits.v_out,
                deposits.created,
                deposits.confirmed,
                tx_outs.block_height,
                tx_outs.address,
                tx_outs.amount_sats
            FROM deposits
            JOIN tx_outs ON deposits.tx_id = tx_outs.tx_id AND deposits.v_out = tx_outs.v_out
            WHERE id = $1 AND user_id = $2"#,
        )
        .bind(id.0)
        .bind(user_id.0)
        .fetch_optional(db)
        .await
        .unwrap()
        .map(|row| row.into_entity())
    }

    pub(super) async fn list_for_user(
        db: &Database,
        user_id: user::Id,
        range: QueryRange,
    ) -> Vec<Deposit> {
        sqlx::query_as::<_, DepositRow>(
            r#"SELECT
                deposits.id,
                deposits.user_id,
                deposits.tx_id,
                deposits.v_out,
                deposits.created,
                deposits.confirmed,
                tx_outs.block_height,
                tx_outs.address,
                tx_outs.amount_sats
            FROM deposits
            JOIN tx_outs ON deposits.tx_id = tx_outs.tx_id AND deposits.v_out = tx_outs.v_out
            WHERE user_id = $1 ORDER BY deposits.created DESC LIMIT $2 OFFSET $3"#,
        )
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

    #[derive(sqlx::FromRow, Debug)]
    struct DepositAddressRow {
        user_id: Uuid,
        token_id: Uuid,
        address: String,
        created: DateTime<Utc>,
    }

    impl DepositAddressRow {
        fn into_entity(self) -> Address {
            Address {
                user_id: user::Id(self.user_id),
                token_id: auth::TokenId(self.token_id),
                address: btc::Address::from_str(&self.address).unwrap(),
                created: self.created,
            }
        }
    }

    #[derive(sqlx::FromRow, Debug)]
    struct DepositRow {
        id: Uuid,
        user_id: Uuid,
        tx_id: String,
        v_out: i32,
        created: DateTime<Utc>,
        confirmed: Option<DateTime<Utc>>,
        block_height: Option<i32>,
        address: String,
        amount_sats: i64,
    }

    impl DepositRow {
        fn into_entity(self) -> Deposit {
            Deposit {
                id: Id(self.id),
                user_id: user::Id(self.user_id),
                tx_out: btc::TxOut {
                    tx: btc::Tx {
                        id: btc::TxId::from_str(&self.tx_id).unwrap(),
                        block_height: self.block_height.map(|x| x.try_into().unwrap()),
                    },
                    address: btc::Address::from_str(&self.address).unwrap(),
                    v_out: self.v_out.try_into().unwrap(),
                    amount: btc::Sats(self.amount_sats),
                },
                created: self.created,
                confirmed: self.confirmed,
            }
        }
    }
}
