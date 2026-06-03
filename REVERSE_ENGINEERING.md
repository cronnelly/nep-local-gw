# 🕵️‍♂️ Reverse Engineering the NEP BDM-400 Telemetry Protocol

This document provides a comprehensive log of the sources, techniques, inspirations, and mathematical breakthroughs used to reverse-engineer the unencrypted HTTP telemetry payload sent by the rebranded **NEP BDM-400** microinverter (Sunflower Balcon 440W kit) to the cloud endpoint `http://www.nepviewer.net/i.php`.

---

## 📖 1. Project Context & Objectives

The goal was to build a fully local, privacy-focused gateway (`nep-gw`) that spoof-responds to the microinverter's cloud upload requests, stores solar telemetry metrics locally, and integrates with **Home Assistant** and **Prometheus** without any cloud dependencies.

### The Target
* **Hardware**: Rebranded Northern Electric Power (NEP) BDM-400 microinverter (400W peak AC output) paired with two series-connected Sunflower 220W monocrystalline panels.
* **Network Behavior**: The inverter issues unencrypted HTTP `POST` requests to `http://www.nepviewer.net/i.php` roughly every 60 seconds.
* **Payload**: A fixed-length binary string of **45 bytes** containing raw electrical measurements.
* **Server Response**: Expects the current server date and time in `YYYYMMDDHHMMSS` format to synchronize the inverter's internal Real-Time Clock (RTC).

---

## 🛠️ 2. Reverse Engineering Methodology & Techniques

We employed several classic reverse-engineering strategies to dissect the binary structure without access to manufacturer documentation:

### A. Raw Packet Capture (Wireshark & TShark)
By intercepting router traffic and capturing `.pcap` files (`pv (2).pcap` to `pv (6).pcap`), we collected payloads across different times of the day (afternoon full load, sunset dusk, nighttime standby, and morning startup). We used `tshark` commands to extract hex data and server response timestamps:
```bash
tshark -r "pv (6).pcap" -T fields -e http.file_data -e frame.time -Y "http.request.method == POST"
```

### B. Python Monotonic & Value Range Scanning
To identify which byte offsets corresponded to specific physical metrics, we wrote custom Python heuristics (`monotonic_search.py` and `search_32bit.py`):
* **Range Scanning**: Searching for 16-bit and 32-bit values that matched real-world bounds (e.g., European grid voltage between `220V` and `240V`, active power, and frequency around `50Hz`).
* **Monotonic Searching**: Scanning the bytes for values that continuously increased or decreased over time to locate accumulators (like uptime or energy).
* **Endianness Scanning**: Automatically checking both Big-Endian (`>`) and Little-Endian (`<`) struct unpacking to match sensor values.

### C. Differential State Analysis
By comparing active generating states against standby/nighttime captures, we isolated constant values (like signature headers, serial number, and firmware version) from dynamic physical parameters (like power, current, and frequency).

---

## 📈 3. Chronological Breakthroughs & Inspirations

Our understanding evolved through several major stages as new capture data arrived:

### Breakthrough 1: Framing, ID, and Serial Number
From the very first payload:
`79 26 00 40 14 ff ff ff ff ff ff ff ff 1c 00 c3 c3 c3 c3 78 56 34 12 00 00 ...`
We identified:
* **Signature**: Byte `0x79` (framing start).
* **Length Fields**: `26 00` (Little-Endian `38` bytes payload) and `1c 00` (Little-Endian `28` bytes data section).
* **Sync Marker**: The repeated hex pattern `c3 c3 c3 c3` (offset 15–18) served as the boundary delimiter.
* **Inverter Serial**: Bytes `78 56 34 12` unpacked as a Little-Endian 32-bit integer: `0x12345678` = `305419896` (matching the hardware label!).

### Breakthrough 2: Checksum Reconstruction
At the end of the 45-byte payload, we noticed two bytes that varied with every change in payload content (e.g. `bb 2e 5b df` vs `43 25 58 f0`). 
By analyzing the math, we discovered a **Dual Checksum** validation system computed on bytes 1 to 42 (inclusive):
1. **Additive Sum**: Byte 43 is a standard modulo 256 addition: $\text{Sum} = \sum_{i=1}^{42} \text{byte}_i \pmod{256}$.
2. **XOR Checksum**: Byte 44 is an exclusive-OR reduction: $\text{XOR} = \text{byte}_1 \oplus \text{byte}_2 \oplus \dots \oplus \text{byte}_{42}$.

### Breakthrough 3: Q-Format Fixed-Point Constants
When analyzing grid voltage and frequency, we observed raw values that were extremely stable but didn't match typical decimal scalings:
* **Grid Frequency**: Raw values like `12798` and `12808` hovered around 50Hz. We realized the sensor operates in a binary fixed-point **Q8 format** (divided by $2^8 = 256$):
  $$12798 / 256.0 = 49.99\text{ Hz}$$
* **Grid Voltage**: Raw values like `5970` and `6006` mapped to European grid voltage. This was mapped using a Q8 format of decivolts (tenths of a volt), resulting in a scaling factor of `/ 25.6`:
  $$5970 / 25.6 = 233.2\text{ V}$$

### Breakthrough 4: The "Sunset Fluke" (NTC Temperature Assumption)
* **The Dilemma**: Yesterday evening, we noticed word `w7` (bytes 37–38) rose continuously from `6748` to `8895` while the solar power dropped to `0W`. Thermodynamic laws dictate that an idle inverter in a cooling evening cannot heat up. Thus, we assumed this was an **NTC (Negative Temperature Coefficient) thermistor** whose raw resistance value increases as temperature drops.
* **The Formula**: We modeled this curve with a highly convincing physical NTC equation: $\text{Temp (°C)} = 125.0 - (\text{raw\_value} / 100.0)$, which perfectly mapped `6748` to `57.52°C` (hot under full load) and `8895` to `36.05°C` (stabilized standby).
* **The Disproof (Morning Reset)**: This morning (cool weather at 24°C), the gateway reported a staggering **124°C**! The raw `w7` had reset to `98`. Plugging `98` into our NTC formula caused the fluke to collapse.
* **The Core Insight**: `w7` was not temperature at all; it was the **Daily Energy** accumulator in Wh, which grew as the sun set yesterday and reset to near-zero when the microinverter booted up this morning!

### Breakthrough 5: Daily Energy & True Temperature Mapped
With the NTC fluke resolved, the final pieces fell into place:
1. **Daily Energy (`w7` at bytes 37-38)**: Natively accumulated Wh scaled by **0.2 Wh per unit** (meaning `energy_wh = raw / 5.0`). Tracking morning captures proved this rose by exactly 2 units (0.4 Wh) in 59 seconds and 12 units (2.4 Wh) in 297 seconds, perfectly matching integrated AC power.
2. **DSP Temperature (`w6` at bytes 35-36)**: The actual physical temperature sensor, scaled simply as `raw / 100.0` °C. This gives a flawless profile: starting at `35.37°C` in the morning, rising to `35.52°C` under load, and peaking at `53.27°C` yesterday afternoon.
3. **DC Voltage Reconstruction**: We determined the packet natively omits DC PV voltage. Using the known DC current (`w4`, scaled `/10.0` A) and AC power, we dynamically reconstruct the series-connected solar panels' voltage using a typical 96% inverter efficiency curve:
   $$\text{DC Voltage (V)} = \min\left(\frac{\text{AC Power (W)}}{0.96 \times \text{DC Current (A)}}, 60.0\text{ V}\right)$$
   This fits perfectly within the absolute $60\text{V}$ physical input limit of the BDM-400 microinverter.

---

## 📐 4. Definitive Data Section Offsets (28 Bytes)

The data section starts at **offset 15** (following the 15-byte header):

| Offset (dec) | Word | Type | Size | Physical Scale | Metric Description |
| :--- | :--- | :--- | :--- | :--- | :--- |
| `15–18` | - | `bytes` | 4 | Constant `0xC3C3C3C3` | Synchronization Marker |
| `19–22` | - | `uint32` (LE) | 4 | Integer | Microinverter Serial Number |
| `23–24` | `w0` | `uint16` (LE) | 2 | Integer | Operating Status (Always `0` = OK) |
| `25–26` | `w1` | `uint16` (LE) | 2 | `/ 100.0` | AC Active Power Output in Watts |
| `27–28` | `w2` | `uint16` (LE) | 2 | `/ 25.6` | Grid AC Voltage in Volts (Q8 decivolts) |
| `29–30` | `w3` | `uint16` (LE) | 2 | `(raw >> 4) / 100` | DSP Reference Voltage in Volts (e.g. `2.56V`) |
| `31–32` | `w4` | `uint16` (BE) | 2 | `/ 10.0` | DC Input Current in Amperes |
| `33–34` | `w5` | `uint16` (LE) | 2 | `/ 256.0` | Grid AC Frequency in Hertz (Q8 Hz) |
| `35–36` | `w6` | `uint16` (LE) | 2 | `/ 100.0` | Inverter DSP Temperature in °C |
| `37–38` | `w7` | `uint16` (LE) | 2 | `/ 5.0` | Daily Energy Accumulator in Watt-hours |
| `39–40` | `w8` | `uint16` (LE) | 2 | Bitmask / State | Firmware Version (High Byte) & State bits (Low Byte) |
| `41–42` | `w9` | `int16` (LE) | 2 | `/ 100.0` | AC Reactive Power in signed VAR |

---

## 🚦 5. Cracked Status Flags & Reference Voltages

Through rigorous state comparisons across dawn, midday, dusk, and deep sleep, we have fully unmasked the bit-level operations of registers `w8` and `w3`:

### A. The Mode & Relay State Register (`w8`)
Word `w8` (bytes 39–40) is structured as follows:
* **High Byte (`0x84`)**: Constant firmware version `v1.32` or similar identifier.
* **Low Byte**: A bitmask representing physical operating states:
  * **Bit 0 (value 0x01) - Sleep Mode**: Inverter is in standby (nighttime/relay open).
  * **Bit 1 (value 0x02) - MPPT Active**: Inverter is actively tracking and converting solar power.
  * **Bit 2 (value 0x04) - AC Relay Closed**: Inverter is physically connected to the home AC grid.

#### Physical System States:
1. **`0x8406`** (`0000 0110`): **Generation Mode** (Relay Closed, MPPT Active, Day mode). Active day and low-light production.
2. **`0x8404`** (`0000 0100`): **Active Standby Mode** (Relay Closed, MPPT Idle, Day mode). Inverter is awake, grid-synchronized, but solar input is 0W.
3. **`0x8401`** (`0000 0001`): **Deep Sleep Mode** (Relay Open, MPPT Idle, Sleep Mode). Night standby mode to prevent grid leakage.

### B. The Internal Reference Voltage (`w3`)
Word `w3` (bytes 29–30) is formatted with:
* **Lower 4 bits**: Status error flags (stay at `0` under normal operation).
* **Upper 12 bits**: A 12-bit ADC value representing the **Inverter 2.5V Internal Analog Reference Voltage (Vref)**.
  To extract Vref, shift right by 4 bits (or divide by 16) and scale `/ 100.0`:
  $$\text{Internal Vref (V)} = \frac{\text{raw\_value} \gg 4}{100.0}$$
  The decoded value varies tightly between `2.54V` and `2.59V`, capturing the precise analog bandgap thermal fluctuations inside the DSP as operating temperatures swing between `35°C` and `53°C`.

---

## 💡 Lessons for Future Reverse-Engineering

* **Avoid confirmation bias on small datasets**: The "NTC temperature formula" was a highly elegant, physically sound hypothesis that perfectly matched yesterday evening's cooling curve. However, it was a complete mathematical illusion caused by the accumulating daily energy.
* **Test at the extremes**: Having captures from a **dawn restart** (reset accumulator, low ambient temperature) was the single key that exposed the coincidence and led to a mathematically unified solution.
* **Look for typical electronics engineering patterns**: Using Q-format binary representations (`/256` and `/25.6`) is an extremely common trick in small microcontrollers to avoid expensive floating-point arithmetic. Identifying this early unlocked several exact physical ranges.

---

## 🤝 5. Credits & Acknowledgements

During the initial research phase, we drew high-level conceptual inspiration from the open-source community, specifically:
*   **[BlinxFox/nep-gw](https://github.com/BlinxFox/nep-gw)**: An ESP32 / WT32-ETH01 based hardware bridge designed to intercept BDM-600/MMI-600 telemetry packets and forward them to Home Assistant and the NEP cloud simultaneously.

### How it helped:
*   **Conceptual Spark**: Seeing that a community project existed to bridge rebranded NEP inverters (like the BDM-600/MMI-600) via local WiFi routing and MQTT verified our architectural approach. It proved that a local gateway spoofing the unencrypted `i.php` cloud endpoint was a highly viable strategy to extract local solar metrics.

### Our Independent Contributions:
While `BlinxFox/nep-gw` provided excellent conceptual validation, we developed the entire byte-level parsing logic, binary structures, dual-checksum verification, Q-format scaling conversions, thermodynamic temperature physics, and daily energy accumulator mathematical proofs **100% independently from scratch** using raw packet captures (`.pcap`) and thermodynamic/electrical physical reasoning.

We highly credit `BlinxFox` for pioneering local integration for NEP-based hardware and providing a great conceptual reference for our Rust gateway architecture!
