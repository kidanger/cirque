use std::time::{Duration, Instant};

#[derive(Debug)]
pub(crate) struct Ping {
    token: Vec<u8>,
    at: Instant,
}

#[derive(Debug)]
pub(crate) struct Pong {
    token: Vec<u8>,
}

#[derive(Debug)]
pub(crate) struct PingState {
    created: Instant,
    timeout: Option<Duration>,
    last_sent: Option<Ping>,
    last_received: Option<Pong>,
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum PingStatus {
    AllGood,
    Timeout(Duration),
    NeedToSend,
}

impl PingState {
    pub(crate) fn new(now: Instant, timeout: Option<Duration>) -> Self {
        Self {
            created: now,
            timeout,
            last_sent: None,
            last_received: None,
        }
    }

    pub(crate) fn on_send_ping(&mut self, token: &[u8], now: Instant) {
        self.last_sent = Some(Ping {
            token: token.to_vec(),
            at: now,
        });
    }

    pub(crate) fn on_receive_pong(&mut self, token: Vec<u8>) {
        self.last_received = Some(Pong { token });
    }

    pub(crate) fn check_status(&self, now: Instant) -> PingStatus {
        let Some(timeout) = self.timeout else {
            return PingStatus::AllGood;
        };

        match (&self.last_sent, &self.last_received) {
            (None, None) => {
                if now - self.created < timeout {
                    PingStatus::AllGood
                } else {
                    PingStatus::NeedToSend
                }
            }
            (None, Some(_)) => PingStatus::NeedToSend,
            (Some(ping), None) => {
                let elapsed = now - ping.at;
                if elapsed < timeout {
                    PingStatus::AllGood
                } else {
                    PingStatus::Timeout(elapsed)
                }
            }
            (Some(ping), Some(pong)) => {
                let elapsed_sent = now - ping.at;

                if elapsed_sent < timeout {
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

    use crate::timeout::PingStatus;

    use super::PingState;

    #[test]
    fn no_timeout() {
        let now = Instant::now();
        let state = PingState::new(now, None);
        assert_eq!(
            state.check_status(now + Duration::from_secs(1000000)),
            PingStatus::AllGood
        );
    }

    #[test]
    fn missing_pong() {
        let timeout = Duration::from_secs(10);
        let now = Instant::now();
        let mut state = PingState::new(now, Some(timeout));
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
        let timeout = Duration::from_secs(10);
        let now = Instant::now();
        let mut state = PingState::new(now, Some(timeout));
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
    }
}
