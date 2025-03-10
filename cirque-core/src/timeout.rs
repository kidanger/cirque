use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct TimeoutConfig {
    pub base_timeout: Duration,
    pub reduced_timeout: Duration,
}

impl TimeoutConfig {
    fn get_timeout(&self, reduced_rate: bool) -> Duration {
        if reduced_rate {
            self.reduced_timeout
        } else {
            self.base_timeout
        }
    }
}

#[derive(Debug)]
pub(crate) struct Ping {
    token: Vec<u8>,
    at: Instant,
}

#[derive(Debug, PartialEq)]
pub(crate) struct Pong {
    token: Vec<u8>,
}

#[derive(Debug)]
pub(crate) struct PingState {
    created: Instant,
    last_sent: Option<Ping>,
    last_received: Option<Pong>,
    timeout_reduction_tokens: u8,
    timeout_config: Option<TimeoutConfig>,
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum PingStatus {
    AllGood,
    Timeout(Duration),
    NeedToSend,
}

impl PingState {
    pub(crate) fn new(now: Instant, timeout_config: Option<TimeoutConfig>) -> Self {
        Self {
            created: now,
            last_sent: None,
            last_received: None,
            timeout_reduction_tokens: 0,
            timeout_config,
        }
    }

    pub(crate) fn on_send_ping(&mut self, token: &[u8], now: Instant) {
        self.last_sent = Some(Ping {
            token: token.to_vec(),
            at: now,
        });
    }

    pub(crate) fn on_receive_pong(&mut self, token: Vec<u8>) {
        let new = Some(Pong { token });
        if self.last_received == new {
            // the user sent pong multiple times, we return early to avoid decreasing the timeout
            // tokens
            return;
        }

        self.last_received = new;

        // decrease the timeout tokens and maybe get back to the normal timeout
        self.timeout_reduction_tokens = self.timeout_reduction_tokens.saturating_sub(1);
    }

    pub(crate) fn aggressively_reduce_timeout(&mut self) {
        // timeout reduction lasts for 10 pings (at reduced_timeout rate)
        self.timeout_reduction_tokens = 10;
    }

    pub(crate) fn check_status(&self, now: Instant) -> PingStatus {
        let Some(timeout_config) = &self.timeout_config else {
            return PingStatus::AllGood;
        };

        let reduced_rate = self.timeout_reduction_tokens != 0;
        let cur_timeout = timeout_config.get_timeout(reduced_rate);

        match (&self.last_sent, &self.last_received) {
            (None, None) => {
                if now - self.created < cur_timeout {
                    PingStatus::AllGood
                } else {
                    PingStatus::NeedToSend
                }
            }
            (None, Some(_)) => PingStatus::NeedToSend,
            (Some(ping), None) => {
                let elapsed = now - ping.at;
                if elapsed < cur_timeout {
                    PingStatus::AllGood
                } else {
                    PingStatus::Timeout(elapsed)
                }
            }
            (Some(ping), Some(pong)) => {
                let elapsed_sent = now - ping.at;

                if elapsed_sent < cur_timeout {
                    // not yet timed-out, nothing to do
                    return PingStatus::AllGood;
                }

                if ping.token == pong.token {
                    // if the time is up, but the token was replied, we need to send a new ping
                    return PingStatus::NeedToSend;
                }

                // the client didn't reply to the ping in time
                PingStatus::Timeout(elapsed_sent)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use crate::{TimeoutConfig, timeout::PingStatus};

    use super::PingState;

    #[test]
    fn no_timeout() {
        let now = Instant::now();
        let mut state = PingState::new(now, None);
        assert_eq!(
            state.check_status(now + Duration::from_secs(1000000)),
            PingStatus::AllGood
        );
        state.aggressively_reduce_timeout();
        assert_eq!(
            state.check_status(now + Duration::from_secs(1000000)),
            PingStatus::AllGood
        );
    }

    #[test]
    fn missing_pong() {
        let timeout_config = TimeoutConfig {
            base_timeout: Duration::from_secs(10),
            reduced_timeout: Duration::from_secs(2),
        };
        let now = Instant::now();
        let mut state = PingState::new(now, Some(timeout_config));
        assert_eq!(state.check_status(now), PingStatus::AllGood);
        let now = now + Duration::from_secs(9);
        assert_eq!(state.check_status(now), PingStatus::AllGood);
        let now = now + Duration::from_secs(2);
        assert_eq!(state.check_status(now), PingStatus::NeedToSend);
        state.on_send_ping(b"token", now);

        let now = now + Duration::from_secs(2);
        assert_eq!(state.check_status(now), PingStatus::AllGood);
        let now = now + Duration::from_secs(9);
        assert_eq!(
            state.check_status(now),
            PingStatus::Timeout(Duration::from_secs(11))
        );
    }

    #[test]
    fn ping_pong() {
        let timeout_config = TimeoutConfig {
            base_timeout: Duration::from_secs(10),
            reduced_timeout: Duration::from_secs(2),
        };
        let now = Instant::now();
        let mut state = PingState::new(now, Some(timeout_config.clone()));
        assert_eq!(state.check_status(now), PingStatus::AllGood);
        let now = now + Duration::from_secs(9);
        assert_eq!(state.check_status(now), PingStatus::AllGood);
        let now = now + Duration::from_secs(2);
        assert_eq!(state.check_status(now), PingStatus::NeedToSend);
        state.on_send_ping(b"token", now);

        let now = now + Duration::from_secs(2);
        assert_eq!(state.check_status(now), PingStatus::AllGood);
        let now = now + Duration::from_secs(7);
        state.on_receive_pong(b"token".to_vec());
        assert_eq!(state.check_status(now), PingStatus::AllGood);
        let now = now + Duration::from_secs(2);
        assert_eq!(state.check_status(now), PingStatus::NeedToSend);
        state.on_send_ping(b"token2", now);

        state.aggressively_reduce_timeout();

        // the ping was not sent under a reduced timeout, but now we want to timeout quickly
        let now = now + Duration::from_secs(8);
        assert_eq!(
            state.check_status(now),
            PingStatus::Timeout(Duration::from_secs(8))
        );
        state.on_receive_pong(b"token2".to_vec());

        let now = now + Duration::from_secs(1);
        assert_eq!(state.check_status(now), PingStatus::NeedToSend);
        let now = now + Duration::from_secs(2);
        assert_eq!(state.check_status(now), PingStatus::NeedToSend);
        state.on_send_ping(b"token3", now);

        let now = now + Duration::from_secs(1);
        assert_eq!(state.check_status(now), PingStatus::AllGood);
        let now = now + Duration::from_secs(2);
        assert_eq!(
            state.check_status(now),
            PingStatus::Timeout(Duration::from_secs(3))
        );
    }
}
