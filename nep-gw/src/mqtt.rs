use nep_protocol::NepTelemetry;
use rumqttc::{AsyncClient, MqttOptions, QoS};
use serde_json::json;
use tokio::sync::mpsc;
use tracing::{error, info, warn};

#[derive(Debug, Clone)]
pub struct MqttConfig {
    pub host: String,
    pub port: u16,
}

/// Dynamic MQTT Worker task using rumqttc
pub async fn run_mqtt_worker(config: MqttConfig, mut rx: mpsc::Receiver<NepTelemetry>) {
    let mqtt_host = config.host;
    let mqtt_port = config.port;

    info!(
        "Initializing MQTT Client connecting to {}:{}...",
        mqtt_host, mqtt_port
    );
    let mut mqttoptions = MqttOptions::new("nep-gateway", mqtt_host, mqtt_port);
    mqttoptions.set_keep_alive(std::time::Duration::from_secs(5));

    let (client, mut eventloop) = AsyncClient::new(mqttoptions, 10);

    // Spawn a connection monitoring task
    tokio::spawn(async move {
        loop {
            match eventloop.poll().await {
                Ok(notification) => {
                    tracing::trace!("MQTT Event: {:?}", notification);
                }
                Err(e) => {
                    warn!("MQTT Connection error: {:?}. Retrying in 5 seconds...", e);
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                }
            }
        }
    });

    let mut discovered = false;

    while let Some(telemetry) = rx.recv().await {
        let serial = format!("{:08x}", telemetry.serial_number);

        // Run Home Assistant MQTT Discovery once on first telemetry capture
        if !discovered {
            info!(
                "Publishing Home Assistant MQTT Discovery configs for serial {}...",
                serial
            );
            if let Err(e) = publish_ha_discovery(&client, &serial).await {
                error!("HA Discovery publish failed: {:?}", e);
            } else {
                discovered = true;
            }
        }

        let dc_power = telemetry.dc_voltage_v * telemetry.dc_current_a;
        let efficiency = if dc_power > 0.0 {
            (telemetry.ac_power_w / dc_power * 100.0).min(100.0)
        } else {
            0.0
        };

        // Publish parsed values in a single clean JSON payload
        let telemetry_payload = json!({
            "ac_power_w": (telemetry.ac_power_w * 100.0).round() / 100.0,
            "ac_voltage_v": (telemetry.ac_voltage_v * 100.0).round() / 100.0,
            "ac_freq_hz": (telemetry.ac_freq_hz * 100.0).round() / 100.0,
            "dc_voltage_v": (telemetry.dc_voltage_v * 100.0).round() / 100.0,
            "dc_current_a": (telemetry.dc_current_a * 100.0).round() / 100.0,
            "dc_power_w": (dc_power * 100.0).round() / 100.0,
            "efficiency_percent": (efficiency * 100.0).round() / 100.0,
            "temperature_c": (telemetry.temp_c * 100.0).round() / 100.0,
            "daily_energy_wh": (telemetry.daily_energy_wh * 100.0).round() / 100.0,
            "reactive_power_var": (telemetry.reactive_power_var * 100.0).round() / 100.0,
            "status": if telemetry.status_code == 0 { "OK" } else { "Error" },
            "error_state": telemetry.error_state_str(),
            "operating_mode": telemetry.operating_mode_str(),
            "version": telemetry.version,
        });

        let topic = format!("nep/telemetry/{}", serial);
        info!("Publishing telemetry JSON to MQTT topic: {}", topic);
        if let Err(e) = client
            .publish(
                topic,
                QoS::AtLeastOnce,
                false,
                telemetry_payload.to_string(),
            )
            .await
        {
            error!("MQTT publish telemetry failed: {:?}", e);
        }
    }
}

async fn publish_ha_discovery(
    client: &AsyncClient,
    serial: &str,
) -> Result<(), rumqttc::ClientError> {
    let device = json!({
        "identifiers": [format!("nep_bdm_{}", serial)],
        "name": format!("NEP BDM-400 ({})", serial),
        "model": "BDM-400",
        "manufacturer": "Northern Electric Power (NEP)"
    });

    let state_topic = format!("nep/telemetry/{}", serial);

    let sensors = vec![
        ("ac_power", "AC Output Power", "power", "W", "ac_power_w"),
        (
            "ac_voltage",
            "AC Grid Voltage",
            "voltage",
            "V",
            "ac_voltage_v",
        ),
        (
            "ac_freq",
            "AC Grid Frequency",
            "frequency",
            "Hz",
            "ac_freq_hz",
        ),
        (
            "dc_voltage",
            "DC PV Voltage",
            "voltage",
            "V",
            "dc_voltage_v",
        ),
        (
            "dc_current",
            "DC PV Current",
            "current",
            "A",
            "dc_current_a",
        ),
        ("dc_power", "DC PV Power", "power", "W", "dc_power_w"),
        (
            "efficiency",
            "Inverter Efficiency",
            "",
            "%",
            "efficiency_percent",
        ),
        (
            "temperature",
            "DSP Temperature",
            "temperature",
            "°C",
            "temperature_c",
        ),
        (
            "daily_energy",
            "Daily Energy",
            "energy",
            "Wh",
            "daily_energy_wh",
        ),
        (
            "reactive_power",
            "Reactive Power",
            "reactive_power",
            "VAR",
            "reactive_power_var",
        ),
        ("error_state", "Error State", "enum", "", "error_state"),
        (
            "operating_mode",
            "Operating Mode",
            "enum",
            "",
            "operating_mode",
        ),
    ];

    for (id, name, dev_class, unit, json_key) in sensors {
        let mut config = json!({
            "name": format!("NEP {} {}", serial, name),
            "state_topic": state_topic,
            "value_template": format!("{{{{ value_json.{} }}}}", json_key),
            "unique_id": format!("nep_{}_{}", serial, id),
            "device": device
        });

        if !dev_class.is_empty() {
            config["device_class"] = json!(dev_class);
        }
        if !unit.is_empty() {
            config["unit_of_measurement"] = json!(unit);
        }

        match id {
            "daily_energy" => config["state_class"] = json!("total_increasing"),
            "efficiency" => {}
            "error_state" => {
                config["options"] = json!([
                    "OK",
                    "Grid Loss / Islanding",
                    "Anti-Islanding Reconnection Sync Timer",
                    "Unknown Error"
                ]);
            }
            "operating_mode" => {
                config["options"] = json!([
                    "Deep Sleep",
                    "Awake Standby",
                    "Active Standby",
                    "Generating",
                    "Unknown State"
                ]);
            }
            _ => config["state_class"] = json!("measurement"),
        }

        let discovery_topic = format!("homeassistant/sensor/nep_{}/{}/config", serial, id);
        client
            .publish(discovery_topic, QoS::AtLeastOnce, true, config.to_string())
            .await?;
    }

    Ok(())
}
