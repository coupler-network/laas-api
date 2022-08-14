use crate::database::Database;
use crate::ln::Lightning;
use crate::worker;
use crate::{btc, ln};
use async_trait::async_trait;
use std::time::Duration;

#[async_trait]
pub trait TxListener: Send {
    /// Processes a tx_out. NOTE: This method may be called multiple times for the same tx_out, and
    /// it should be prepared to handle that.
    async fn process(&mut self, tx_out: &btc::TxOut);
}

/// Starts a tx listener.
pub async fn listen(
    start_height: u32,
    db: &Database,
    lightning: &Lightning,
    listener: impl TxListener + 'static,
) {
    worker::start(Worker {
        chain_tip: queries::get_chain_tip(start_height, db).await,
        node: lightning.create_node().await,
        listener,
    });
}

struct Worker<L> {
    chain_tip: u32,
    node: ln::Node,
    listener: L,
}

#[async_trait]
impl<L: TxListener + 'static> worker::Worker for Worker<L> {
    async fn run(&mut self) {
        loop {
            let tx_outs = self
                .node
                .get_tx_outs(ln::TransactionsQuery {
                    start_height: self.chain_tip,
                    num_blocks: 10,
                })
                .await;
            log::info!(
                "tx listener going through blocks {} to {}, number of transactions: {}",
                self.chain_tip,
                self.chain_tip + 10,
                tx_outs.len()
            );
            for tx_out in tx_outs.iter() {
                self.listener.process(tx_out).await;
            }
            let new_chain_tip = tx_outs
                .into_iter()
                .flat_map(|tx_out| tx_out.tx.block_height)
                .max();
            match new_chain_tip {
                Some(new_chain_tip) => self.chain_tip = new_chain_tip + 1,
                None => return,
            }
        }
    }

    fn timeout() -> Duration {
        Duration::from_secs(10)
    }
}

mod queries {
    use crate::database::{self, Database};

    pub(super) async fn get_chain_tip(start_height: u32, db: &Database) -> u32 {
        sqlx::query_as::<_, database::MaxRow<i32>>("SELECT MAX(block_height) AS max FROM tx_outs")
            .fetch_one(db)
            .await
            .unwrap()
            .max
            .unwrap_or(start_height.try_into().unwrap())
            .try_into()
            .unwrap()
    }
}
