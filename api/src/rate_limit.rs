use std::sync::Arc;

use app::user;
use dashmap::{mapref::entry::Entry, DashMap};
use std::time::Duration;

pub struct RateLimit {
    limit: usize,
    span: Duration,
    counter: Arc<DashMap<user::Id, usize>>,
}

impl RateLimit {
    pub fn new(limit: usize, span: Duration) -> Self {
        Self {
            limit,
            span,
            counter: Arc::new(Default::default()),
        }
    }

    /// Returns true if the user should be rate limited, false otherwise.
    pub fn limit(&self, user_id: user::Id) -> bool {
        match self.counter.entry(user_id) {
            Entry::Occupied(mut count) => {
                let count = count.get_mut();
                if *count >= self.limit {
                    true
                } else {
                    *count += 1;
                    self.decrement_later(user_id);
                    false
                }
            }
            Entry::Vacant(e) => {
                e.insert(0);
                false
            }
        }
    }

    fn decrement_later(&self, user_id: user::Id) {
        let counter = Arc::clone(&self.counter);
        let span = self.span;
        tokio::spawn(async move {
            tokio::time::sleep(span).await;
            match counter.entry(user_id) {
                Entry::Occupied(mut e) => {
                    let v = e.get_mut();
                    *v -= 1;
                    if *v == 0 {
                        e.remove();
                    }
                }
                Entry::Vacant(_) => {
                    log::error!(
                        "entry should not be vacant, this is a bug. user id {:?}",
                        user_id
                    );
                }
            }
        });
    }
}
