use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    time::Instant,
};

struct Stats {
    fuel: f64,
    /// fuel added per second
    fill_rate: f64,
    tank_size: f64,
    cost_per_connection: f64,
    max_fill_rate: f64,
    cost_multiplier: f64,
    last_refill: Instant,
}

impl Stats {
    fn refill(&mut self, now: Instant) {
        let added_fuel = now.duration_since(self.last_refill).as_secs_f64() * self.fill_rate;
        self.fuel = (self.fuel + added_fuel).max(self.tank_size);
        self.last_refill = now;
    }

    fn consume_one(&mut self) -> bool {
        if self.fuel < self.cost_per_connection {
            // no maximum
            self.fill_rate /= self.cost_multiplier;
            return false;
        }

        self.fuel -= self.cost_per_connection;
        self.fill_rate = (self.fill_rate * self.cost_multiplier).max(self.max_fill_rate);
        true
    }
}

impl Default for Stats {
    fn default() -> Self {
        Self {
            fuel: 1_000.,
            tank_size: 1_000.,
            cost_per_connection: 1_000.,
            fill_rate: 100.,
            max_fill_rate: 100.,
            cost_multiplier: 1.2,
            last_refill: Instant::now(),
        }
    }
}

pub(crate) struct ConnectionValidator {
    stats: HashMap<IpAddr, Stats>,
}

impl ConnectionValidator {
    pub(crate) fn new() -> Self {
        Self {
            stats: Default::default(),
        }
    }

    pub(crate) fn validate(&mut self, peer_addr: SocketAddr) -> Result<(), std::io::Error> {
        let now = Instant::now();
        let ip = peer_addr.ip();
        let stats = self.stats.entry(ip).or_default();

        stats.refill(now);

        if !stats.consume_one() {
            return Err(std::io::Error::other(format!(
                "connection from {ip} dropped due to poor stats"
            )));
        }

        // clean-up the hashmap to free space
        // it is only done on successful connection, so it is less triggered when getting spammed
        self.stats.retain(|_, stats| {
            stats.refill(now);
            stats.fuel != stats.tank_size || stats.fill_rate != stats.max_fill_rate
        });

        Ok(())
    }
}
