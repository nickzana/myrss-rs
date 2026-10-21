use crate::AppState;
use std::sync::Arc;

pub fn spawn_scheduler(state: Arc<AppState>) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(
            state.config.feed.refresh_minutes * 60,
        ));
        loop {
            interval.tick().await;
            state.fetcher.refresh_due(&state.pool).await;
            if let Some(days) = state.config.server.retention_days {
                let _ = crate::db::retain(&state.pool, days).await;
            }
        }
    });
}
