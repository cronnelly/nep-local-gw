# CLAUDE.md — NEP BDM-800 compatibility notes

Context for anyone (including Claude Code) continuing work on this repo. This
file records how the upstream `nep-local-gw` project — written for the **NEP
BDM-400** single-input microinverter — was extended to also accept the **NEP
BDM-800** dual-MPPT microinverter, and what remains unverified.

The findings below come from analysing a single live BDM-800 telemetry packet
capture (`POST /i.php` to `www.nepviewer.net`), decoded against this project's
documented 45-byte layout and its 31 bundled BDM-400 sample packets in
`parse_payload.py`. The serial number in the packet below has been anonymized
to `0xDEADBEEF` (bytes 19–22) with both checksums recomputed; every other byte
is as captured.

## The captured BDM-800 packet

```
Body (45 bytes, hex):
79 26 00 40 14 00 00 0f 0f 0f 0f 00 00 1c 00 c3 c3 c3 c3 ef be ad de
00 00 bd 56 cb 17 20 10 26 2a b0 31 70 13 ab 12 05 8c 05 e0 22 70
```

Decoded AC-side values (all physically sane for a 230 V / 50 Hz grid,
confirming the byte offsets are genuinely aligned and not coincidentally
passing):

| Field            | Value       | Notes                          |
| ---------------- | ----------- | ------------------------------ |
| Serial           | 0xDEADBEEF  | bytes 19–22, u32 LE (anonymized) |
| AC power         | 222.05 W    | bytes 25–26, u16 LE / 100      |
| AC voltage       | 237.93 V    | bytes 27–28, u16 LE / 25.6     |
| Grid frequency   | 49.688 Hz   | bytes 33–34, u16 LE / 256      |
| DSP temperature  | 49.76 °C    | bytes 35–36, u16 LE / 100      |
| Daily energy     | 955.8 Wh    | bytes 37–38, u16 LE / 5        |
| Reactive power   | -81.87 VAR  | bytes 41–42, i16 LE / 100      |
| Op-mode low byte | 0x05        | "Active Standby"               |

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

For the captured packet: 3.8 A + 4.2 A = **8.0 A** total, which at ~30 V ≈ 240 W
DC → 222 W AC ≈ 92% efficiency. Physically consistent.

Why this is safe for existing BDM-400 users: on a BDM-400, byte 31 is always
`0x00`, so `ch1 == 0.0`, `ch2 == the original single-string value`, and
`total == the old value`. The two pre-existing unit tests (`test_parse_payload1`,
`test_parse_payload6`) still pass unchanged, and `test_parse_payload6`'s
`dc_current_a == 2.6` assertion still holds.

### 3. Sync-marker hard-reject after inverter reset (now fixed)

On the first report after the BDM-800 was reset (2026-07-03), bytes 15–18 —
normally the `0xC3C3C3C3` data-section sync marker — read `FF FF FF FF`. Both
checksums validated and every telemetry field decoded physically sane (447 W AC
midday, daily accumulator freshly cleared to 233.8 Wh), but
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
- **Home Assistant device model** is now configurable via `INVERTER_MODEL` env
  var / `--model` flag (default `BDM-400`), used in `publish_ha_discovery`'s
  device name and `model` fields. Still to do: thread the model into
  `parse_payload` / the DC-side sensor set (see the DC voltage point above) —
  the flag currently only changes HA metadata, not parsing behaviour.
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
