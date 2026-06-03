mod handlers;
mod metrics;
mod mqtt;

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
}

pub struct AppConfig {
    pub port: u16,
    pub mqtt: mqtt::MqttConfig,
}

impl AppConfig {
    pub fn from_env() -> Self {
        let port = std::env::var("PORT")
            .unwrap_or_else(|_| "80".to_string())
            .parse::<u16>()
            .unwrap_or(80);

        let mqtt_host = std::env::var("MQTT_HOST").unwrap_or_else(|_| "::1".to_string());
        let mqtt_port = std::env::var("MQTT_PORT")
            .unwrap_or_else(|_| "1883".to_string())
            .parse::<u16>()
            .unwrap_or(1883);

        Self {
            port,
            mqtt: mqtt::MqttConfig {
                host: mqtt_host,
                port: mqtt_port,
            },
        }
    }
}

#[tokio::main]
async fn main() {
    // Initialize logging
    tracing_subscriber::fmt::init();
    info!("Starting Local NEP Microinverter Gateway...");

    // Retrieve all configurations from the environment
    let config = AppConfig::from_env();

    // Register Prometheus metrics
    metrics::register_metrics();

    // Create a channel to bridge HTTP requests to the MQTT worker
    let (tx, rx) = mpsc::channel::<NepTelemetry>(100);
    let app_state = Arc::new(AppState { tx });

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
