use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    time::Instant,
};

#[derive(Debug)]
struct Config {
    /// A connection consumes this amount of credits.
    cost_per_connection: f64,
    /// Each failed connection (not enough credits) divides the fill_rate by this amount.
    /// Each successful connection multiplies the fill_rate by this amount.
    cost_multiplier: f64,
    /// Maximum fill rate for the credits (in credits per second). The actual value associated to
    /// an IP can only be lower or equal than this value.
    max_fill_rate: f64,
    /// Maximum number of credits that an IP can have at any moment. This is also the default
    /// value for IP that never connected.
    max_credits: f64,
}

/// In the case of a bad actor (spam in a short amount of time), their fill rate will be
/// reduced at each failed connection (i.e. most connections).
/// There might be times where their credits are maxed out: in this case the client is no longer
/// penalized (the fill rate is reset). This is fair because the client had to wait for potentially
/// a long time to get to this point, and if it continues spamming then the fill rate will quickly
/// reduce again.
#[derive(Debug)]
struct Stats {
    credits: f64,
    fill_rate: f64,
    last_refill: Instant,
}

impl Stats {
    fn new(config: &Config) -> Self {
        Self {
            credits: config.max_credits,
            fill_rate: config.max_fill_rate,
            last_refill: Instant::now(),
        }
    }

    fn refill(&mut self, config: &Config, now: Instant) {
        let added_fuel = now.duration_since(self.last_refill).as_secs_f64() * self.fill_rate;
        self.credits += added_fuel;
        if self.credits >= config.max_credits {
            self.credits = config.max_credits;
            self.fill_rate = config.max_fill_rate;
        }
        self.last_refill = now;
    }

    fn consume_one(&mut self, config: &Config) -> bool {
        if self.credits < config.cost_per_connection {
            // Reduce the fill rate.
            // The limit to the fill rate is here to ensure that the fill rate stays positive (so
            // that that eventually, the client will have enough credits to get accepted).
            self.fill_rate = (self.fill_rate / config.cost_multiplier).max(0.01);
            return false;
        }

        self.credits -= config.cost_per_connection;
        self.fill_rate = (self.fill_rate * config.cost_multiplier).min(config.max_fill_rate);
        true
    }
}

#[derive(Debug)]
pub(crate) struct ConnectionValidator {
    stats: HashMap<IpAddr, Stats>,
    config: Config,
}

impl ConnectionValidator {
    pub(crate) fn new() -> Self {
        Self {
            stats: Default::default(),
            config: Config {
                cost_per_connection: 1_000.,
                cost_multiplier: 1.618,
                max_fill_rate: 100.,
                max_credits: 1_000.,
            },
        }
    }

    fn validate_at_time(
        &mut self,
        peer_addr: SocketAddr,
        now: Instant,
    ) -> Result<(), std::io::Error> {
        let config = &self.config;
        let ip = peer_addr.ip();
        let stats = self.stats.entry(ip).or_insert_with(|| Stats::new(config));

        stats.refill(config, now);

        if !stats.consume_one(config) {
            return Err(std::io::Error::other(format!(
                "connection from {ip} dropped due to poor stats"
            )));
        }

        // clean-up the hashmap to free space
        // it is only done on successful connection, so it is less triggered when getting spammed
        self.stats.retain(|_, stats| {
            stats.refill(config, now);
            // The stats is removed if and only if the IP has max credits.
            stats.credits != config.max_credits
        });

        Ok(())
    }

    pub(crate) fn validate(&mut self, peer_addr: SocketAddr) -> Result<(), std::io::Error> {
        self.validate_at_time(peer_addr, Instant::now())
    }
}

#[cfg(test)]
mod tests {
    use std::{
        net::SocketAddr,
        str::FromStr,
        time::{Duration, Instant},
    };

    use super::ConnectionValidator;

    #[test]
    fn test1() {
        let mut validator = ConnectionValidator::new();
        let ip1 = SocketAddr::from_str("10.0.0.1:12340").unwrap();
        let ip2 = SocketAddr::from_str("10.0.0.2:12340").unwrap();

        let t0 = Instant::now();
        let t1 = t0 + Duration::from_secs(1);
        let t20 = t0 + Duration::from_secs(20);

        validator.validate_at_time(ip1, t0).unwrap();
        validator.validate_at_time(ip1, t1).unwrap_err();
        validator.validate_at_time(ip2, t1).unwrap();
        validator.validate_at_time(ip1, t20).unwrap();
    }
}
