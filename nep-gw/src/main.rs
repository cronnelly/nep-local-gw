mod handlers;
mod metrics;
mod mqtt;
mod upstream;

use axum::{
    routing::{get, post},
    Router,
};
use nep_protocol::NepTelemetry;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::info;

pub struct AppState {
    pub tx: mpsc::Sender<NepTelemetry>,
    /// `Some` when dual-delivery mode is enabled (--forward-upstream):
    /// inverter POSTs are also relayed to the real NEP cloud.
    pub upstream: Option<upstream::UpstreamForwarder>,
}

pub struct AppConfig {
    pub port: u16,
    pub mqtt: mqtt::MqttConfig,
    pub upstream: Option<upstream::UpstreamConfig>,
}

impl AppConfig {
    /// Configuration comes from the environment (like everything else in this
    /// project); the dual-delivery switch is also exposed as CLI flags for
    /// convenience. Flags win over env vars.
    pub fn load() -> Self {
        let port = std::env::var("PORT")
            .unwrap_or_else(|_| "80".to_string())
            .parse::<u16>()
            .unwrap_or(80);

        let mqtt_host = std::env::var("MQTT_HOST").unwrap_or_else(|_| "::1".to_string());
        let mqtt_port = std::env::var("MQTT_PORT")
            .unwrap_or_else(|_| "1883".to_string())
            .parse::<u16>()
            .unwrap_or(1883);
        let mqtt_credentials = match std::env::var("MQTT_USERNAME") {
            Ok(username) => Some((
                username,
                std::env::var("MQTT_PASSWORD").unwrap_or_default(),
            )),
            Err(_) => {
                if std::env::var("MQTT_PASSWORD").is_ok() {
                    eprintln!("MQTT_PASSWORD is set but MQTT_USERNAME is not; ignoring it");
                }
                None
            }
        };

        let mut forward_upstream = std::env::var("FORWARD_UPSTREAM")
            .map(|v| matches!(v.to_lowercase().as_str(), "1" | "true" | "yes" | "on"))
            .unwrap_or(false);
        let mut upstream_url = std::env::var("UPSTREAM_URL")
            .unwrap_or_else(|_| upstream::DEFAULT_UPSTREAM_URL.to_string());
        let mut model = std::env::var("INVERTER_MODEL").unwrap_or_else(|_| "BDM-400".to_string());

        let mut args = std::env::args().skip(1);
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--forward-upstream" => forward_upstream = true,
                "--upstream-url" => match args.next() {
                    Some(url) => upstream_url = url,
                    None => {
                        eprintln!("--upstream-url requires a value");
                        std::process::exit(2);
                    }
                },
                "--model" => match args.next() {
                    Some(m) => model = m,
                    None => {
                        eprintln!("--model requires a value (e.g. BDM-400, BDM-800)");
                        std::process::exit(2);
                    }
                },
                other => {
                    eprintln!(
                        "Unknown argument: {}\n\nUsage: nep-gw [--forward-upstream] [--upstream-url <url>] [--model <name>]\n\
                         \n  --forward-upstream   Also relay inverter POSTs to the real NEP cloud\n\
                         \n  --upstream-url <url> Upstream endpoint (default: {})\n\
                         \n  --model <name>       Inverter model shown in Home Assistant (default: BDM-400)",
                        other,
                        upstream::DEFAULT_UPSTREAM_URL
                    );
                    std::process::exit(2);
                }
            }
        }

        Self {
            port,
            mqtt: mqtt::MqttConfig {
                host: mqtt_host,
                port: mqtt_port,
                credentials: mqtt_credentials,
                model,
            },
            upstream: forward_upstream.then_some(upstream::UpstreamConfig { url: upstream_url }),
        }
    }
}

#[tokio::main]
async fn main() {
    // Initialize logging
    tracing_subscriber::fmt::init();
    info!("Starting Local NEP Microinverter Gateway...");

    // Retrieve all configurations from the environment / CLI flags
    let config = AppConfig::load();

    // Register Prometheus metrics
    metrics::register_metrics();

    let forwarder = config.upstream.clone().map(|cfg| {
        info!(
            "Dual-delivery mode enabled: relaying inverter POSTs to {}",
            cfg.url
        );
        upstream::UpstreamForwarder::new(cfg)
    });

    // Create a channel to bridge HTTP requests to the MQTT worker
    let (tx, rx) = mpsc::channel::<NepTelemetry>(100);
    let app_state = Arc::new(AppState {
        tx,
        upstream: forwarder,
    });

    // Start MQTT task in the background
    let mqtt_config = config.mqtt.clone();
    tokio::spawn(async move {
        mqtt::run_mqtt_worker(mqtt_config, rx).await;
    });

    // Build the Axum router
    let app = Router::new()
        .route("/i.php", post(handlers::handle_inverter_post))
        .route("/metrics", get(handlers::handle_metrics))
        .with_state(app_state);

    let addr = format!("[::]:{}", config.port);
    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    info!("HTTP Server listening on {}...", addr);
    axum::serve(listener, app).await.unwrap();
}
