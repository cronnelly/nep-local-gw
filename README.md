# 🕵️‍♂️ Local NEP Microinverter Gateway & Telemetry Parser

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-2021-orange.svg)](https://www.rust-lang.org/)
[![Docker](https://img.shields.io/badge/docker-alpine-blue.svg)](Dockerfile)

A local, privacy-focused gateway (`nep-gw`) and protocol parser (`nep-protocol`) designed to intercept and parse telemetry payloads sent by rebranded **NEP BDM-400** microinverters (such as those in Sunflower Balcon 440W kits). 

It operates by spoofing the cloud endpoint `http://www.nepviewer.net/i.php`, parsing the unencrypted binary telemetry packets locally, and forwarding the decoded measurements directly to **Home Assistant (via MQTT)** and **Prometheus**.

---

## 🚀 Features

* **100% Local**: Run your solar balcony kit without cloud dependencies.
* **Axum-based HTTP Gateway**: Light and performant HTTP server implementing Time Sync responses expected by the inverter RTC.
* **Auto-Discovery in Home Assistant**: Automatic sensor creation in Home Assistant via MQTT Discovery (voltage, power, daily energy, temperature, frequency, reactive power, status).
* **Prometheus Metrics**: Scraping endpoint `/metrics` for custom Grafana dashboards.
* **NOM Parser**: Fast, safe binary parsing of the 45-byte payload implemented in Rust.
* **DC Voltage & Power Reconstruction**: Dynamically estimates DC PV current, voltage, and panel power using standard inverter efficiency curves (since the inverter natively omits DC PV voltage from its uploads).

---

## 📐 The Telemetry Protocol

The microinverter uploads unencrypted HTTP `POST` requests to `/i.php` containing a fixed-length **45-byte** binary payload. The structure has been fully reverse-engineered:

### 45-Byte Packet Layout

| Offset (dec) | Size (bytes) | Type | Scale / Format | Metric Description |
| :--- | :--- | :--- | :--- | :--- |
| `0` | 1 | `uint8` | Constant `0x79` | Packet framing start signature |
| `1–2` | 2 | `uint16` (LE) | `38` | Payload length from offset 5 to 42 |
| `3–4` | 2 | `uint16` (BE) | `0x4014` | Fixed command identifier |
| `5–12` | 8 | `bytes` | `0xFF` padding | Gateway/AP identifier padding |
| `13–14` | 2 | `uint16` (LE) | `28` | Length of data section (offset 15 to 42) |
| `15–18` | 4 | `bytes` | `0xC3C3C3C3` | Data section synchronization header |
| `19–22` | 4 | `uint32` (LE) | Hex Integer | Inverter Serial Number |
| `23–24` | 2 | `uint16` (LE) | `0` | General status / padding |
| `25–26` | 2 | `uint16` (LE) | `/ 100.0` (W) | AC Active Power Output in Watts |
| `27–28` | 2 | `uint16` (LE) | `/ 25.6` (V) | Grid AC Voltage in Volts (Q8 decivolts) |
| `29–30` | 2 | `uint16` (LE) | Bitmask / Vref | Internal flags and DSP Reference Voltage |
| `31–32` | 2 | `uint16` (BE) | `/ 10.0` (A) | DC Input Current in Amperes |
| `33–34` | 2 | `uint16` (LE) | `/ 256.0` (Hz) | Grid AC Frequency in Hertz (Q8 Hz) |
| `35–36` | 2 | `uint16` (LE) | `/ 100.0` (°C) | DSP Temperature in Celsius |
| `37–38` | 2 | `uint16` (LE) | `/ 5.0` (Wh) | Daily Energy Accumulator in Wh (0.2 Wh / unit) |
| `39–40` | 2 | `uint16` (LE) | Bitmask | Firmware Version & Relay/State bits |
| `41–42` | 2 | `int16` (LE) | `/ 100.0` (VAR) | AC Reactive Power in signed Volt-Amperes Reactive |
| `43` | 1 | `uint8` | sum % 256 | **Additive Checksum** (bytes 1 to 42) |
| `44` | 1 | `uint8` | XOR reduction | **XOR Checksum** (bytes 1 to 42) |

---

## 🛠️ Getting Started

### Prerequisites

* Rust compiler toolchain (Edition 2021) or Docker installed.
* An MQTT broker (e.g. Mosquitto) for Home Assistant integration.
* DNS hijacking/spoofing setup on your local network (e.g., DNS spoofing in MikroTik or dnsmasq) to redirect `www.nepviewer.net` to the machine hosting `nep-gw`.

### Environment Configuration

The gateway is configured via the following environment variables:

| Variable | Description | Default |
| :--- | :--- | :--- |
| `PORT` | The port the HTTP server binds to | `80` |
| `MQTT_HOST` | Hostname/IP of your MQTT broker | `::1` |
| `MQTT_PORT` | Port of your MQTT broker | `1883` |
| `RUST_LOG` | Tracing logging level (`info`, `debug`, `error`) | `info` |

---

## 🐳 Docker Deployment

The gateway is packaged in an optimized Alpine-based multi-stage Dockerfile that supports compilation for multi-arch targets (including `aarch64` ARM targets like Raspberry Pi).

### Build Image
```bash
docker build -t nep-gw .
```

### Run Container
```bash
docker run -d \
  --name nep-gw \
  -p 80:80 \
  -e PORT=80 \
  -e MQTT_HOST=192.168.1.50 \
  -e MQTT_PORT=1883 \
  -e RUST_LOG=info \
  --restart unless-stopped \
  nep-gw
```

---

## 🦀 Rust Workspace Structure

The project is structured as a Cargo workspace containing:
1. **`nep-protocol`**: A standalone Rust library that uses `nom` to parse the 45-byte telemetry payloads, validate the dual checksums, and perform scaling conversions.
2. **`nep-gw`**: The HTTP gateway daemon written with `axum`. It intercepts `/i.php`, calls the parser library, updates Prometheus metrics, and manages the MQTT client loop for Home Assistant discovery.

### Building locally
```bash
cargo build --release
```

### Running unit tests
```bash
cargo test
```

---

## 🔌 Integrations

### Home Assistant MQTT Discovery
On receiving telemetry from a new microinverter serial number, the gateway automatically publishes MQTT Discovery configuration topics to `homeassistant/sensor/nep_<serial_number>/...`.
Sensors exposed:
* `AC Power` (W)
* `AC Voltage` (V)
* `Grid Frequency` (Hz)
* `DC Current` (A)
* `DC Voltage` (V, reconstructed)
* `DC Power` (W, reconstructed)
* `Temperature` (°C)
* `Daily Energy` (Wh)
* `Reactive Power` (VAR)
* `Inverter Status` (operating state)

### Prometheus Scraping
The gateway exposes a standard Prometheus `/metrics` endpoint on the configured HTTP port. Metrics exported:
* `nep_packets_received_total`
* `nep_ac_power_watts`
* `nep_ac_voltage_volts`
* `nep_dc_current_amperes`
* `nep_ac_frequency_hertz`
* `nep_dc_voltage_volts`
* `nep_temperature_celsius`
* `nep_daily_energy_watthours`
* `nep_reactive_power_var`

---

## 📜 License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

## 🤝 Credits

Special thanks to the community efforts, particularly **[BlinxFox/nep-gw](https://github.com/BlinxFox/nep-gw)**, for providing the initial hardware references and inspiration for local BDM-600/MMI-600 telemetry redirection.
