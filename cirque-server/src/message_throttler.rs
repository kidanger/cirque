use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub(crate) struct MessageThrottler {
    last_timestamp: Instant,
    threshold: Duration,
}

impl MessageThrottler {
    pub(crate) fn new(max_messages_per_second: u32) -> Self {
        Self {
            last_timestamp: Instant::now(),
            threshold: Duration::from_secs(1) / max_messages_per_second,
        }
    }

    pub(crate) async fn maybe_slow_down(&mut self) {
        let elapsed = self.last_timestamp.elapsed();
        if elapsed < self.threshold {
            let delay = self.threshold - elapsed;
            tokio::time::sleep(delay).await;
        }
        self.last_timestamp = Instant::now();
    }
}
