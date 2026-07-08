# CLAUDE.md — NEP BDM-800 compatibility notes

Context for anyone (including Claude Code) continuing work on this repo. This
file records how the upstream `nep-local-gw` project — written for the **NEP
BDM-400** single-input microinverter — was extended to also accept the **NEP
BDM-800** dual-MPPT microinverter, and what remains unverified.

The initial findings came from analysing a single live BDM-800 telemetry
packet capture (`POST /i.php` to `www.nepviewer.net`), decoded against this
project's documented 45-byte layout and its 31 bundled BDM-400 sample packets
in `parse_payload.py`. They were later extended by calibrating a live BDM-800
against an independent reference meter (see the calibration section). The
serial number in the packet below has been anonymized to `0xDEADBEEF`
(bytes 19–22) with both checksums recomputed; every other byte is as captured.

## The captured BDM-800 packet

```
Body (45 bytes, hex):
79 26 00 40 14 00 00 0f 0f 0f 0f 00 00 1c 00 c3 c3 c3 c3 ef be ad de
00 00 bd 56 cb 17 20 10 26 2a b0 31 70 13 ab 12 05 8c 05 e0 22 70
```

Decoded values, using the calibrated BDM-800 scales established below:

| Field            | Value       | Notes                                          |
| ---------------- | ----------- | ---------------------------------------------- |
| Serial           | 0xDEADBEEF  | bytes 19–22, u32 LE (anonymized)               |
| AC power         | 282.72 W    | bytes 25–26, u16 LE / 25π (see calibration)    |
| Voltage word     | raw 6091    | bytes 27–28 — internal bus, NOT grid RMS (see below) |
| Grid frequency   | 49.688 Hz   | bytes 33–34, u16 LE / 256 (device clock reads ~0.35% low) |
| DSP temperature  | 49.76 °C    | bytes 35–36, u16 LE / 100                      |
| Daily energy     | 1103.0 Wh   | bytes 37–38, u16 LE × 0.2308 (see calibration) |
| Reactive power   | −81.87 VAR  | bytes 41–42, i16 LE / 100 — decode unreliable (see w9) |
| Op-mode low byte | 0x05        | "Generating" on the BDM-800                    |

Both dual checksums (additive `[43]`, XOR `[44]`, computed over bytes 1..42)
**validate**. Packet integrity was never the problem.

## What was broken, and why

### 1. Header hard-reject on the Gateway/AP ID field (BLOCKING — now fixed)

`nep-protocol`'s `parse_header()` used `nom`'s `tag()` — an exact-match
combinator — for bytes 5..12:

```rust
// old:
let (input, _) = tag([0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF])(input)?;
```

The BDM-400 emits `FF FF FF FF FF FF FF FF` there. The **BDM-800 emits
`00 00 0f 0f 0f 0f 00 00`**, so `tag` failed and `parse_payload()` returned
`Err("Header parsing failed")` for every BDM-800 packet — even though the packet
and both checksums were valid.

This field is a gateway/AP identifier placeholder carrying **no telemetry**. The
fix skips it instead of matching it:

```rust
// new:
let (input, _) = take(8usize)(input)?;
```

### 2. DC current exploded to ~977 A (now fixed, but see HYPOTHESIS below)

The parser read DC current as one big-endian u16 over bytes 31–32, scaled `/10`:

```rust
let (input, dc_current_raw) = be_u16(input)?;  // still used to consume the bytes
let dc_current_a = dc_current_raw as f64 / 10.0;
```

Across **all 31 BDM-400 sample packets** in `parse_payload.py`, byte 31 (the BE
high byte) is **always `0x00`** — a 400 W single-input unit never draws enough
DC current for it to be non-zero. On the BDM-800, byte 31 is `0x26`, so
`be_u16/10` gave `0x262A / 10 = 977.0 A`, which is impossible.

The BDM-800 is a **dual-MPPT (two independent PV inputs)** device, whereas the
BDM-400 is single-input. The fix decodes the two bytes as independent per-string
currents at `/10 A`:

```rust
let dc_current_ch1_a = (dc_current_raw >> 8)   as f64 / 10.0; // byte 31
let dc_current_ch2_a = (dc_current_raw & 0xFF) as f64 / 10.0; // byte 32
let dc_current_a     = dc_current_ch1_a + dc_current_ch2_a;   // total
```

For the captured packet: 3.8 A + 4.2 A = **8.0 A** total. Physically
consistent with the calibrated AC power.

Why this is safe for existing BDM-400 users: on a BDM-400, byte 31 is always
`0x00`, so `ch1 == 0.0`, `ch2 == the original single-string value`, and
`total == the old value`. The two pre-existing unit tests (`test_parse_payload1`,
`test_parse_payload6`) still pass unchanged, and `test_parse_payload6`'s
`dc_current_a == 2.6` assertion still holds.

### 3. Sync-marker hard-reject after inverter reset (now fixed)

On the first report after the BDM-800 was reset (2026-07-03), bytes 15–18 —
normally the `0xC3C3C3C3` data-section sync marker — read `FF FF FF FF`. Both
checksums validated and every telemetry field decoded sane, but
`parse_data_section`'s exact `tag([0xC3; 4])` rejected the packet. Presumably
the same "uninitialized placeholder" semantics as the 0xFF Gateway/AP field on
the BDM-400. It is NOT a first-report transient: a report ~80 minutes after
the reset still carried `FF FF FF FF`, so it may persist indefinitely (or
until some yet-unidentified event). Fixed the same way as the
Gateway/AP field: `take(4usize)` instead of the tag. Packet integrity relies on
the dual checksums, which `parse_payload()` validates before parsing;
`test_parse_payload_rejects_bad_checksums` pins that guard, and
`test_parse_payload_bdm800_post_reset_sync_marker` pins the captured packet
(serial anonymized, checksums recomputed).

## Calibration against a reference meter (2026-07-05..07)

A live BDM-800 (reporting every ~10 min) was calibrated against a **Shelly
Outdoor PlugS Gen3** in the same socket (sub-minute sampling), using three
days of Home Assistant history and 205 raw packets from the gateway logs.
Results, implemented in `nep_protocol::InverterModel`:

### AC power (bytes 25–26): `raw / 25π`, not `raw / 100`

The BDM-400's `/100` under-reads the BDM-800 by a flat **×1.273** — constant
from 20 W to 620 W (per-day means 1.271/1.284/1.273; weighted fit 1.2742,
median 1.2729), i.e. a pure scale error statistically indistinguishable from
**4/π = 1.27324**. Adopted scale: `raw / (25π) ≈ raw / 78.54`. Verified
end-to-end after deployment: Shelly/NEP ratio 1.002.

### Daily energy (bytes 37–38): `raw × 0.2308 Wh`, not `raw / 5`

Fitted over clean monotonic counter segments vs the reference meter
(1.1507–1.1569 × the documented 0.2; pooled 0.2301–0.2314, ≈ 3/13). Note
1.273 / 1.154 ≈ 1.10 — the same ~10% power-vs-energy internal inconsistency
present in the BDM-400 sample packets (integration of the 31 samples implies
≈ 0.1796 Wh/unit vs the documented 0.2), so **the BDM-400 may share the ×4/π
power error** — unverified without a reference meter on a real BDM-400 (and
it would push the reconstructed DC voltage past the 60 V input spec, so
something else would have to be off too).

### Mode byte: 0x05 = Generating on the BDM-800

The unit emits 0x05 all day while producing at full power. The documented
bit0/bit1/bit2 mask does not transfer across models; `operating_mode_str()`
is model-aware.

### Daily counter semantics

Resets at **dawn**, not midnight. After a *full (DC-side) power loss* it
warps back to a stale flash-persisted value (observed 1061→283 Wh and
701→100 Wh following manual resets), and the inverter takes a long time to
rejoin the network. An **AC-only interruption does not warp it** (tested:
mains unplugged ~2 min with panels connected; the counter continued
2554.0→2841.4 Wh and production resumed within ~4 min, though telemetry
reporting paused for ~40 min). The controller is DC-powered. In normal
unattended operation the counter is monotonic and consistent.

### The voltage word (bytes 27–28) is an internal bus voltage, not grid RMS

Established by ratioing all 205 logged packets against the reference meter,
which held a steady 236–246 V throughout:

- **Idle / near-zero power (P < ~5 W)**: reads 0.72 × V_rms (n=26), i.e.
  essentially **V_peak / 2** (1/√2 = 0.707) — the resting value of an
  internal node passively charged to the grid peak (e.g. the DC link through
  the unfolder diodes). A repeated raw `0x1110` ≈ "170.6 V" is this resting
  value — not flags and not a frozen sensor.
- **Generating (P > ~250 W)**: varies smoothly and *inversely* with output
  power — ratio ≈ 1.141 − 0.000318·P_W (rms residual 0.04, consistent across
  days): 1.06 × V_rms at 265 W, 1.00 × at ~440 W, 0.93 × at 675 W. Plausibly
  a regulated internal bus that droops with load.
- **Transition band (~20–250 W)**: flips bistably between the two branches.

The original reverse engineering identified this word as "grid voltage /25.6"
because it happens to cross 1.0 × V_rms in the 400–550 W band where the
daytime BDM-400 captures were validated. The BDM-400's out-of-band readings
(259–265 V idle, "155.4 V" during a grid event) are the same phenomenon,
possibly with different internal setpoints. Consequences: grid voltage is
likely not present in the payload at all; the HA discovery sensor is named
"Internal Bus Voltage" for the BDM-800 (the BDM-400 keeps its historical
label pending verification against a reference meter).

### Grid frequency: `/256` is correct — do not "recalibrate" it

The BDM-800 reads ~0.36% low and the Shelly Outdoor Gen3 ~0.4% low, but a
third meter in the same installation reads a normal 49.99–50.06 Hz, and
voltage-sag cross-correlation proves all circuits share one utility feed.
Both solar-side devices simply have slow internal oscillators (frequency is
counted against each device's own clock). `/256` also matches the BDM-400
sample captures exactly.

### w9 (bytes 41–42) is almost certainly not signed reactive power

In the BDM-400 samples, relay-open deep-sleep packets "report" −194…−301 VAR
(impossible with an open relay), and treating w9 as *unsigned with u16
wrap-around* makes it monotonic in AC power (≈ `45237 + 146.6 × P_W` on one
day's series) with a temperature-drifting zero-power baseline. Semantics
unknown; the sensor still publishes the signed decode — treat it as
meaningless.

## Thermal derating: the inverter self-limits at ≈57 °C DSP temperature

Observed during calibration and confirmed by intervention:

- Output moves on a **quantized limiter ladder** (~88 W ≈ 11% of 800 W:
  264 / 352 / 440 / 526 / 616 W …), stepping down instantly on disturbance
  and recovering ~1 level per 1–2 minutes with brief probe excursions.
- With the unit mounted in a poorly ventilated spot, the DSP climbs all
  morning and then sits **pinned at 57.0 ± 0.15 °C** for the afternoon while
  output holds the highest ladder level whose heat balance fits (~525 W in
  ~35 °C ambient). Step-downs trigger at ≥57.1 °C readings, recoveries at
  ≤57.0 °C — a thermostat implemented via the ladder. The throttle engaged at
  nearly the same minute on consecutive days (deterministic heat soak).
- **Confirmed by intervention (2026-07-07)**: re-mounting the inverter off
  the hot surface with a small fan kept the DSP ≤49.9 °C; the limiter never
  engaged, output followed a smooth solar curve to a sustained ~695–698 W,
  and daily production rose ~17%. Mounting and airflow are therefore the
  first thing to check when a BDM-800 plateaus below its rating in summer.
- Separately, heavy appliance inrush on the same circuit (e.g. an A/C
  compressor start, ~3 kW) knocks the limiter down one or more levels; with
  the slow recovery this costs ~10 min of reduced output per event.

## ⚠️ HYPOTHESIS — confirm before trusting per-channel DC values

The byte→channel mapping (**byte 31 = channel 1, byte 32 = channel 2**) is
inferred from a **single** BDM-800 packet. It is physically plausible but
unconfirmed. The **total** (`dc_current_a`) is the robust figure; the individual
`ch1`/`ch2` split may be wrong (swapped, different scale, or not per-channel at
all). To confirm:

- Capture a series across varying irradiance (partial shading on one panel is
  ideal — it should make the two channel bytes diverge independently).
- Check that each channel's current tracks that string's expected production and
  that `ch1 + ch2` stays consistent with AC power / efficiency.
- Cross-check against the official NEP app's per-panel readings if available.

## Still model-divergent / not addressed (good next tasks)

- **DC voltage & DC power reconstruction.** `dc_voltage_v` is *reconstructed*,
  not measured: `min(ac_power_w / (0.96 * dc_current_a), 60.0)`. Both the 0.96
  efficiency constant and the **60 V cap** are BDM-400-specific (its 2×220 W
  series-panel topology, ~48–54 V Voc). For a dual-MPPT BDM-800 each channel has
  its own DC voltage, so a single reconstructed voltage from summed current is
  not physically meaningful. The gateway still computes it (nothing breaks) but
  **do not trust `dc_voltage_v`/`dc_power_w`/efficiency on the BDM-800.** Reverse
  the real per-channel DC voltage if the packet carries it, or drop these
  sensors for the 800.
- **`INVERTER_MODEL` / `--model` selects parsing behaviour** (2026-07-05):
  it is parsed into `nep_protocol::InverterModel` (lenient, e.g. `bdm800`
  accepted; unrecognized names warn and fall back to BDM-400 scales) and
  threaded through `AppState` into `parse_payload(&body, model)`. It selects
  the AC-power and daily-energy scales and the model-aware
  `operating_mode_str()`, in addition to the HA discovery metadata. **A
  BDM-800 deployment must set `INVERTER_MODEL=BDM-800` (or `--model`), or all
  power/energy values will be ~21–27% low.** The DC-side sensor set is still
  shared (see the DC voltage point above).
- **Model auto-detection.** There is no reliable in-packet model field yet.
  Byte 31 ≠ 0x00 is a weak hint (fails at night/low light when the 800 may also
  read 0x00). The Gateway/AP field pattern (`00 00 0f 0f 0f 0f 00 00` vs 0xFF)
  is a stronger candidate — investigate whether it is stable per model.
- The `version` string (`version_raw` at bytes 39–40 formatted as `x.yy`) is
  almost certainly a **misinterpretation**: `REVERSE_ENGINEERING.md` treats this
  word (`w8`) as a relay/MPPT/day **bitmask**, and `operating_mode_str()` reads
  its low byte as a state enum. The decimal "version" is likely meaningless.

## Deployment notes (not code — environmental)

- `nep-gw` **spoofs** `www.nepviewer.net`. By default it does **not** forward
  upstream, so pointing the inverter at it stops the official NEP app/portal
  from updating. **Dual-delivery is now available**: run with
  `--forward-upstream` (or `FORWARD_UPSTREAM=true`) and every raw inverter POST
  is also relayed fire-and-forget to the real cloud
  (`UPSTREAM_URL`/`--upstream-url`, default `http://www.nepviewer.net/i.php`).
  See `nep-gw/src/upstream.rs`. The gateway host must resolve the *real*
  `www.nepviewer.net` (scope the DNS spoof to the inverter); if it can't, pin
  the IP via `--upstream-url` (the `Host: www.nepviewer.net` header is always
  sent). Relays carry an `X-NEP-GW-Relay` marker header and marked incoming
  requests are never re-forwarded, so a DNS loop can't cause infinite relaying.
  The inverter always gets the locally generated time-sync response — the relay
  result is only logged and counted (`nep_upstream_forwards_total`,
  `nep_upstream_forward_errors_total`).
- Redirect the inverter by spoofing DNS for `www.nepviewer.net` → the gateway
  host (e.g. dnsmasq), and make sure port 80 on that host is free.
- The gateway's RTC time-sync response (local time as `YYYYMMDDHHMMSS`) matches
  what the real cloud returns, so the inverter is satisfied by it.
- Switching an existing BDM-800 install to the calibrated scales step-changes
  the HA history (~+27% power, ~+15% energy at the boundary); deploy after
  sunset to avoid a phantom mid-day jump in the `total_increasing`
  daily-energy statistic. HA entity `unique_id`s are serial-based, so setting
  the model does not orphan existing entities — only the displayed
  device/model name and the voltage sensor's label change.

## What this patch changed

- `nep-protocol/src/lib.rs`
  - `take` added to the `nom` imports.
  - `parse_header`: strict `tag([0xFF; 8])` → `take(8usize)` for the Gateway/AP
    field.
  - `NepTelemetry`: added `dc_current_ch1_a`, `dc_current_ch2_a`; `dc_current_a`
    is now the per-channel sum.
  - Added `test_parse_payload_bdm800` (captured packet, serial anonymized).
    Existing BDM-400 tests unchanged and still passing.
- `nep-gw/src/metrics.rs`: added `DC_CURRENT_CH1` / `DC_CURRENT_CH2` Prometheus
  gauges (+ registration). `DC_CURRENT` is now documented as the total.
- `nep-gw/src/handlers.rs`: set the two new gauges per packet.
- `nep-gw/src/mqtt.rs`: added `dc_current_ch1_a` / `dc_current_ch2_a` to the
  telemetry JSON and two matching HA discovery sensors (`DC PV Current CH1/CH2`).

Verified: `cargo test -p nep-protocol` → 3/3 pass; `cargo check --workspace` →
clean.

## What the 2026-07 calibration patches changed

- `nep-protocol/src/lib.rs`: added `InverterModel` (`Bdm400` default, `Bdm800`)
  with `from_name()` lenient parsing and the model-specific
  `scale_power_w` / `scale_energy_wh` (BDM-800: `raw/(25π)` W, `raw×0.2308`
  Wh). `parse_payload` now takes the model; `NepTelemetry` carries it and
  `operating_mode_str()` maps BDM-800 `0x05` → "Generating". BDM-400 decoding
  is bit-for-bit unchanged (guarded by the pre-existing tests).
- `nep-gw`: `AppConfig`/`AppState` carry the parsed `InverterModel`;
  `handlers.rs` passes it to `parse_payload`. Unrecognized `--model` names log
  a warning and use BDM-400 scales while still displaying the given string in
  HA. `mqtt.rs` labels the voltage sensor "Internal Bus Voltage" on the
  BDM-800 (same `unique_id`/JSON key, so existing HA entities and history are
  preserved).
- `nep-gw.service` + `README.md`: documented that the model flag selects
  parsing scales, and the corrected byte-layout rows.
- Tests: BDM-800 fixtures updated to the calibrated expectations
  (282.72 W / 1102.99 Wh and 569.49 W / 269.81 Wh); added
  `test_inverter_model_from_name`. `cargo test --workspace` → 6/6 pass.
