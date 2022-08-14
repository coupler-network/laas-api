use std::{error::Error, future::Future, time::Duration};
use thiserror::Error;

#[derive(Debug, Error)]
#[error("concurrency conflict")]
pub struct ConflictError;

const MAX_RETRIES: u64 = 10;

/// This function implements a retry loop for concurrency conflicts. It will keep retrying the
/// callback as long as the callback returns an error whose chain includes [`ConflictError`]. If
/// [`MAX_RETRIES`] are exceeded, the function will panic.
pub async fn retry_loop<F: Future<Output = Result<T, E>>, T, E: Error + 'static>(
    mut cb: impl FnMut() -> F,
) -> Result<T, E> {
    for i in 1..MAX_RETRIES {
        match cb().await {
            Ok(result) => return Ok(result),
            Err(e) if is_conflict(Some(&e)) => {
                let timeout = Duration::from_secs(i);
                log::info!("got a conflict error, sleeping for {:?}", timeout);
                tokio::time::sleep(timeout).await;
            }
            Err(e) => return Err(e),
        }
    }
    cb().await
}

fn is_conflict(e: Option<&(dyn Error + 'static)>) -> bool {
    e.map(|e| e.is::<ConflictError>() || is_conflict(e.source()))
        .unwrap_or(false)
}
