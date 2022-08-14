use crate::{
    auth, balance, btc, concurrency,
    database::Database,
    ln::{self, Lightning},
    seconds::Seconds,
    swallow_panic, worker, CashLimits, QueryRange,
};
use async_trait::async_trait;
use futures::StreamExt;
use std::time::Duration;
use tokio::sync::Mutex;

mod entities;

pub use entities::{Error, Id, Invoice, Settlement};

pub async fn create(
    grant: &auth::ReceiveGrant,
    db: &Database,
    node: &mut ln::Node,
    amount: btc::MilliSats,
    memo: Option<String>,
    expiry: Seconds,
    limits: &CashLimits,
) -> Result<Invoice, Error> {
    let daily_total = queries::daily_total(db, grant.user_id).await;
    let invoice = Invoice::create(grant, node, amount, memo, expiry, limits, daily_total).await?;

    let mut data_tx = db.begin().await.unwrap();
    queries::upsert(&mut data_tx, &invoice).await;
    data_tx.commit().await.unwrap();
    Ok(invoice)
}

pub async fn get(grant: &auth::ReadGrant, db: &Database, id: Id) -> Option<Invoice> {
    queries::get(db, id, grant.user_id).await
}

pub async fn list(grant: &auth::ReadGrant, db: &Database, range: QueryRange) -> Vec<Invoice> {
    queries::list(db, grant.user_id, range).await
}

pub async fn start_worker(db: Database, lightning: &Lightning) {
    let mut node = lightning.create_node().await;
    {
        let mut uncompleted_invoices = queries::get_unsettled(&db);
        while let Some(invoice) = uncompleted_invoices.next().await {
            if let ln::InvoiceStatus::Settled(settled_invoice) =
                node.get_invoice_status(&invoice.raw).await
            {
                complete(&db, invoice, &settled_invoice).await;
            }
        }
    }
    worker::start(InvoiceListener { db, node });
}

struct InvoiceListener {
    db: Database,
    node: ln::Node,
}

#[async_trait]
impl worker::Worker for InvoiceListener {
    async fn run(&mut self) {
        let settle_index = queries::get_max_settle_index(&self.db).await;
        let mut stream = self.node.stream_settled_invoices(settle_index).await;
        while let Some(settled_invoice) = stream.next().await {
            swallow_panic(async {
                match queries::get_by_invoice(&self.db, &settled_invoice.raw).await {
                    Some(invoice) => complete(&self.db, invoice, &settled_invoice).await,
                    None => {
                        log::info!(
                            "invoice {:?} is not a user invoice, skipping",
                            settled_invoice.raw.0
                        );
                    }
                }
            })
            .await;
        }
    }

    fn timeout() -> Duration {
        Duration::from_secs(5)
    }
}

async fn complete(db: &Database, invoice: Invoice, settled_invoice: &ln::SettledInvoice) {
    let invoice = Mutex::new(invoice);
    concurrency::retry_loop(|| async {
        let mut invoice = invoice.lock().await;
        if !invoice.is_settled() {
            let mut data_tx = db.begin().await.unwrap();
            let mut balance = balance::get(&mut data_tx, invoice.user_id).await;
            invoice.settle(&mut balance, settled_invoice);
            queries::upsert(&mut data_tx, &invoice).await;
            balance::update(&mut data_tx, &balance).await?;
            data_tx.commit().await.unwrap();
        }
        Ok::<_, concurrency::ConflictError>(())
    })
    .await
    .unwrap();
}

mod queries {
    use super::{Id, Invoice, Settlement};
    use crate::{
        auth, btc,
        database::{self, Database, SumRow},
        ln, user, QueryRange,
    };
    use chrono::{DateTime, Duration, Utc};
    use const_format::formatcp;
    use futures::{stream::BoxStream, StreamExt};
    use uuid::Uuid;

    const COLUMNS: &str = "id, user_id, token_id, amount_msats, memo, invoice, created, expiration, settlement_amount, settlement_timestamp, settle_index";

    pub(super) async fn upsert(data_tx: &mut database::Transaction, invoice: &Invoice) {
        sqlx::query(
            formatcp!(r#"INSERT INTO invoices ({})
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11) ON CONFLICT (id) DO UPDATE SET
                user_id = $2, token_id = $3, amount_msats = $4, memo = $5, invoice = $6, created = $7, expiration = $8, settlement_amount = $9, settlement_timestamp = $10, settle_index = $11"#,
                COLUMNS)
        )
        .bind(invoice.id.0)
        .bind(invoice.user_id.0)
        .bind(invoice.token_id.0)
        .bind(invoice.amount.0)
        .bind(invoice.memo.clone())
        .bind(invoice.raw.0.clone())
        .bind(invoice.created)
        .bind(invoice.expiration)
        .bind(invoice.settlement.as_ref().map(|settlement| settlement.amount.0))
        .bind(invoice.settlement.as_ref().map(|settlement| settlement.timestamp))
        .bind(invoice.settlement.as_ref().map(|settlement| i64::try_from(settlement.settle_index).unwrap()))
        .execute(&mut *data_tx)
        .await
        .unwrap();
    }

    pub(super) async fn get_by_invoice(db: &Database, invoice: &ln::RawInvoice) -> Option<Invoice> {
        sqlx::query_as::<_, InvoiceRow>(formatcp!(
            "SELECT {} FROM invoices WHERE invoice = $1",
            COLUMNS
        ))
        .bind(invoice.0.clone())
        .fetch_optional(db)
        .await
        .unwrap()
        .map(|row| row.into_entity())
    }

    pub(super) async fn get(db: &Database, id: Id, user_id: user::Id) -> Option<Invoice> {
        sqlx::query_as::<_, InvoiceRow>(formatcp!(
            "SELECT {} FROM invoices WHERE id = $1 AND user_id = $2",
            COLUMNS
        ))
        .bind(id.0)
        .bind(user_id.0)
        .fetch_optional(db)
        .await
        .unwrap()
        .map(|row| row.into_entity())
    }

    pub(super) async fn list(db: &Database, user_id: user::Id, range: QueryRange) -> Vec<Invoice> {
        sqlx::query_as::<_, InvoiceRow>(formatcp!(
            "SELECT {} FROM invoices WHERE user_id = $1 ORDER BY created DESC LIMIT $2 OFFSET $3",
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

    pub(super) fn get_unsettled(db: &Database) -> BoxStream<'_, Invoice> {
        sqlx::query_as::<_, InvoiceRow>(formatcp!(
            "SELECT {} FROM invoices WHERE settlement_timestamp IS NULL",
            COLUMNS
        ))
        .fetch(db)
        .map(|row| row.unwrap().into_entity())
        .boxed()
    }

    pub(super) async fn get_max_settle_index(db: &Database) -> u64 {
        sqlx::query_as::<_, database::MaxRow<i64>>("SELECT MAX(settle_index) AS max FROM invoices")
            .fetch_one(db)
            .await
            .unwrap()
            .max
            .unwrap_or(0)
            .try_into()
            .unwrap()
    }

    pub(super) async fn daily_total(db: &Database, user_id: user::Id) -> btc::MilliSats {
        sqlx::query_as::<_, SumRow<Option<i64>>>(
            "SELECT SUM(CAST(amount_msats AS INTEGER)) AS sum FROM invoices WHERE user_id = $1 AND created > $2",
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
    struct InvoiceRow {
        id: Uuid,
        user_id: Uuid,
        token_id: Uuid,
        amount_msats: i64,
        memo: Option<String>,
        invoice: String,
        created: DateTime<Utc>,
        expiration: DateTime<Utc>,
        settlement_amount: Option<i64>,
        settlement_timestamp: Option<DateTime<Utc>>,
        settle_index: Option<i64>,
    }

    impl InvoiceRow {
        fn into_entity(self) -> Invoice {
            Invoice {
                id: Id(self.id),
                user_id: user::Id(self.user_id),
                token_id: auth::TokenId(self.token_id),
                amount: btc::MilliSats(self.amount_msats),
                memo: self.memo,
                raw: ln::RawInvoice(self.invoice),
                created: self.created,
                expiration: self.expiration,
                settlement: match (
                    self.settlement_amount,
                    self.settlement_timestamp,
                    self.settle_index,
                ) {
                    (Some(amount), Some(timestamp), Some(settle_index)) => Some(Settlement {
                        amount: btc::MilliSats(amount),
                        timestamp,
                        settle_index: settle_index.try_into().unwrap(),
                    }),
                    _ => None,
                },
            }
        }
    }
}
