use axum::{
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
};
use chrono::Local;
use nep_protocol::parse_payload;
use prometheus::{Encoder, TextEncoder};
use std::sync::Arc;
use tracing::{error, info, warn};

use crate::metrics::{
    AC_FREQ, AC_POWER, AC_VOLTAGE, DAILY_ENERGY, DC_CURRENT, DC_CURRENT_CH1, DC_CURRENT_CH2,
    DC_VOLTAGE, PACKETS_RECEIVED, REACTIVE_POWER, REGISTRY, TEMPERATURE,
};
use crate::upstream::RELAY_MARKER_HEADER;
use crate::AppState;

pub async fn handle_inverter_post(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<String, StatusCode> {
    let hex_payload: String = body.iter().map(|b| format!("{:02x}", b)).collect();
    info!(
        "Received POST /i.php from microinverter. Size: {} bytes. Raw hex payload: {}",
        body.len(),
        hex_payload
    );

    // Dual-delivery: relay the raw body to the real NEP cloud in the
    // background, even if our own parser rejects it (the cloud may understand
    // packet types we don't). Requests already carrying the relay marker are
    // our own forwards bounced back (misconfigured DNS) — never re-forward
    // those, or a single packet would loop forever.
    if let Some(forwarder) = &state.upstream {
        if headers.contains_key(RELAY_MARKER_HEADER) {
            warn!(
                "Received a request carrying {} — the upstream URL resolves back to this \
                 gateway (DNS loop). Not forwarding.",
                RELAY_MARKER_HEADER
            );
        } else {
            let forwarder = forwarder.clone();
            let body = body.clone();
            tokio::spawn(async move { forwarder.forward(body).await });
        }
    }

    match parse_payload(&body) {
        Ok(telemetry) => {
            info!(
                "Successfully parsed telemetry from serial: {:08x}",
                telemetry.serial_number
            );

            // Update Prometheus metrics
            PACKETS_RECEIVED.inc();
            AC_POWER.set(telemetry.ac_power_w);
            AC_VOLTAGE.set(telemetry.ac_voltage_v);
            DC_CURRENT.set(telemetry.dc_current_a);
            DC_CURRENT_CH1.set(telemetry.dc_current_ch1_a);
            DC_CURRENT_CH2.set(telemetry.dc_current_ch2_a);
            AC_FREQ.set(telemetry.ac_freq_hz);
            DC_VOLTAGE.set(telemetry.dc_voltage_v);
            TEMPERATURE.set(telemetry.temp_c);
            DAILY_ENERGY.set(telemetry.daily_energy_wh);
            REACTIVE_POWER.set(telemetry.reactive_power_var);

            // Forward to MQTT task
            if let Err(e) = state.tx.send(telemetry).await {
                error!("Failed to forward telemetry to MQTT worker: {:?}", e);
            }

            // Return current local time YYYYMMDDHHMMSS to sync the inverter RTC
            let time_str = Local::now().format("%Y%m%d%H%M%S").to_string();
            info!(
                "Responding to inverter with time synchronization string: {}",
                time_str
            );
            Ok(time_str)
        }
        Err(err) => {
            warn!("Failed to parse payload: {}", err);
            Err(StatusCode::BAD_REQUEST)
        }
    }
}

pub async fn handle_metrics() -> String {
    let mut buffer = Vec::new();
    let encoder = TextEncoder::new();
    let metric_families = REGISTRY.gather();
    encoder.encode(&metric_families, &mut buffer).unwrap();
    String::from_utf8(buffer).unwrap()
}
