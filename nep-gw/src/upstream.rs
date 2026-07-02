use axum::body::Bytes;
use std::time::Duration;
use tracing::{info, warn};

use crate::metrics::{UPSTREAM_FORWARD_ERRORS, UPSTREAM_FORWARDS};

/// Marker header attached to every relayed request. If an incoming request
/// already carries it, the gateway is (directly or indirectly) receiving its
/// own relay — e.g. the host's DNS for www.nepviewer.net is also spoofed back
/// to us — and the handler must not forward it again.
pub const RELAY_MARKER_HEADER: &str = "x-nep-gw-relay";

/// The real NEP cloud endpoint the microinverter was originally posting to.
pub const DEFAULT_UPSTREAM_URL: &str = "http://www.nepviewer.net/i.php";

/// The vhost the NEP cloud expects. Sent explicitly so that pinning the
/// upstream URL to a raw IP (when this host's own DNS is spoofed too) still
/// reaches the right site.
const UPSTREAM_HOST: &str = "www.nepviewer.net";

#[derive(Clone, Debug)]
pub struct UpstreamConfig {
    pub url: String,
}

/// Relays raw inverter POSTs to the real NEP cloud (dual-delivery mode).
///
/// Cheap to clone: `reqwest::Client` is an `Arc` internally.
#[derive(Clone)]
pub struct UpstreamForwarder {
    client: reqwest::Client,
    url: String,
}

impl UpstreamForwarder {
    pub fn new(config: UpstreamConfig) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .expect("failed to build upstream HTTP client");
        Self {
            client,
            url: config.url,
        }
    }

    /// Relay one raw inverter payload upstream. Fire-and-forget: callers spawn
    /// this so the inverter's time-sync response never waits on the cloud, and
    /// a cloud outage cannot affect local operation.
    pub async fn forward(&self, body: Bytes) {
        let size = body.len();
        let result = self
            .client
            .post(&self.url)
            .header(reqwest::header::HOST, UPSTREAM_HOST)
            .header(RELAY_MARKER_HEADER, "1")
            .body(body)
            .send()
            .await;

        match result {
            Ok(response) => {
                let status = response.status();
                let reply = response.text().await.unwrap_or_default();
                if status.is_success() {
                    UPSTREAM_FORWARDS.inc();
                    info!(
                        "Relayed {} bytes to upstream {} -> {} (reply: {:?})",
                        size, self.url, status, reply
                    );
                } else {
                    UPSTREAM_FORWARD_ERRORS.inc();
                    warn!(
                        "Upstream {} rejected relayed packet: {} (reply: {:?})",
                        self.url, status, reply
                    );
                }
            }
            Err(err) => {
                UPSTREAM_FORWARD_ERRORS.inc();
                warn!("Failed to relay packet to upstream {}: {}", self.url, err);
            }
        }
    }
}
