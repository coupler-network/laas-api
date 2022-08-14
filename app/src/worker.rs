use async_trait::async_trait;
use std::time::Duration;

use crate::swallow_panic;

#[async_trait]
pub trait Worker: Send {
    async fn run(&mut self);
    fn timeout() -> Duration;
}

pub fn start<W: Worker + 'static>(mut worker: W) {
    tokio::spawn(async move {
        loop {
            swallow_panic(worker.run()).await;
            tokio::time::sleep(W::timeout()).await;
        }
    });
}
