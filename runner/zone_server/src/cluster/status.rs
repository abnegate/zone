use metrics::Gauge;
use tokio::sync::watch;

/// Whether the bus's pub/sub connection is subscribed, mirrored into the
/// `zone_cluster_bus_connected` gauge when the bus reports one.
#[derive(Debug)]
pub struct Status {
    connected: watch::Sender<bool>,
    gauge: Option<Gauge>,
}

impl Status {
    pub fn new(gauge: Option<Gauge>) -> Self {
        if let Some(gauge) = &gauge {
            gauge.set(0.0);
        }
        Self {
            connected: watch::Sender::new(false),
            gauge,
        }
    }

    pub fn set(&self, connected: bool) {
        self.connected.send_replace(connected);
        if let Some(gauge) = &self.gauge {
            gauge.set(if connected { 1.0 } else { 0.0 });
        }
    }

    pub fn connected(&self) -> bool {
        *self.connected.borrow()
    }

    pub fn watch(&self) -> watch::Receiver<bool> {
        self.connected.subscribe()
    }
}
