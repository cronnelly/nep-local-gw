use nom::{
    bytes::complete::{tag, take},
    number::complete::{be_u16, le_i16, le_u16, le_u32},
    IResult,
};
use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct NepTelemetry {
    pub serial_number: u32,
    pub status_code: u16,
    pub ac_power_w: f64,
    pub ac_voltage_v: f64,
    pub operating_flags: u16,
    /// Total DC input current in Amperes (sum of both MPPT channels).
    pub dc_current_a: f64,
    /// DC input current, MPPT channel 1 (byte 31). Always 0 on single-input BDM-400.
    pub dc_current_ch1_a: f64,
    /// DC input current, MPPT channel 2 (byte 32). The single string on BDM-400.
    pub dc_current_ch2_a: f64,
    pub ac_freq_hz: f64,
    pub dc_voltage_v: f64,
    pub temp_c: f64,
    pub daily_energy_wh: f64,
    pub version: String,
    pub version_raw: u16,
    pub reactive_power_var: f64,
}

impl NepTelemetry {
    /// Decodes the w0 status code into a human-readable error state string.
    pub fn error_state_str(&self) -> &'static str {
        match self.status_code {
            0x0000 => "OK",
            0x0020 => "Grid Loss / Islanding",
            0x0004 => "Anti-Islanding Reconnection Sync Timer",
            _ => "Unknown Error",
        }
    }

    /// Decodes the w8 low byte into a human-readable operating mode string.
    pub fn operating_mode_str(&self) -> &'static str {
        let w8_low = self.version_raw & 0xFF;
        match w8_low {
            0x01 => "Deep Sleep",
            0x04 => "Awake Standby",
            0x05 => "Active Standby",
            0x06 => "Generating",
            _ => "Unknown State",
        }
    }
}

/// Validates the dual checksums at the end of a 45-byte payload.
/// Checksum is calculated on bytes 1 to 42 (inclusive).
pub fn validate_checksums(data: &[u8]) -> bool {
    if data.len() < 45 {
        return false;
    }
    let body = &data[1..43];
    let sum: u8 = body.iter().fold(0u8, |acc, &b| acc.wrapping_add(b));
    let xor: u8 = body.iter().fold(0u8, |acc, &b| acc ^ b);
    data[43] == sum && data[44] == xor
}

fn parse_header(input: &[u8]) -> IResult<&[u8], ()> {
    // 0: Signature 'y' (0x79)
    let (input, _) = tag([0x79])(input)?;
    // 1-2: Length (0x0026 = 38 bytes)
    let (input, _) = tag([0x26, 0x00])(input)?;
    // 3-4: Cmd type (0x4014)
    let (input, _) = tag([0x40, 0x14])(input)?;
    // 5-12: Gateway / AP identifier (8 bytes). This is NOT a fixed constant.
    // The BDM-400 emits 0xFF padding here, but the BDM-800 emits a different
    // value (observed: `00 00 0f 0f 0f 0f 00 00`). The original
    // `tag([0xFF; 8])` therefore hard-rejected every non-BDM-400 device with
    // "Header parsing failed", even though the packet and its checksums were
    // valid. Skip the field instead of matching it. See CLAUDE.md.
    let (input, _) = take(8usize)(input)?;
    // 13-14: Data Length (0x001c = 28 bytes)
    let (input, _) = tag([0x1C, 0x00])(input)?;
    Ok((input, ()))
}

fn parse_data_section(input: &[u8]) -> IResult<&[u8], NepTelemetry> {
    // 15-18: Sync marker. Usually 0xC3C3C3C3, but after an inverter reset a
    // live BDM-800 was observed emitting 0xFFFFFFFF here — presumably an
    // "uninitialized" placeholder, just like the 0xFF Gateway/AP field on the
    // BDM-400. The old exact match rejected such packets even though their
    // checksums were valid. Packet integrity is already guaranteed by the dual
    // checksums validated in parse_payload(), so skip the field instead of
    // matching it. See CLAUDE.md.
    let (input, _) = take(4usize)(input)?;
    // 19-22: Serial number
    let (input, serial_number) = le_u32(input)?;
    // 23-24: Status code
    let (input, status_code) = le_u16(input)?;
    // 25-26: AC Power
    let (input, ac_power_raw) = le_u16(input)?;
    // 27-28: AC Voltage
    let (input, ac_voltage_raw) = le_u16(input)?;
    // 29-30: Operating flags
    let (input, operating_flags) = le_u16(input)?;
    // 31-32: DC Current (BE)
    let (input, dc_current_raw) = be_u16(input)?;
    // 33-34: AC Grid Frequency
    let (input, ac_freq_raw) = le_u16(input)?;
    // 35-36: Temperature
    let (input, temp_raw) = le_u16(input)?;
    // 37-38: Daily Energy
    let (input, daily_energy_raw) = le_u16(input)?;
    // 39-40: Version
    let (input, version_raw) = le_u16(input)?;
    // 41-42: Reactive Power (signed)
    let (input, reactive_power_raw) = le_i16(input)?;

    let ac_power_w = ac_power_raw as f64 / 100.0;
    let ac_voltage_v = ac_voltage_raw as f64 / 25.6;
    // DC input current. On the single-input BDM-400 the high byte (offset 31)
    // is always 0x00 and the low byte (offset 32) carries the single string's
    // current at /10 A, so the historical `be_u16 / 10` worked. The dual-MPPT
    // BDM-800 populates BOTH bytes (one per string), which made `be_u16 / 10`
    // explode to a nonsensical ~977 A.
    //
    // We instead decode the two bytes as independent per-string currents at
    // /10 A. This is exact for the BDM-400 (ch1 == 0, so the total equals the
    // old value and the existing unit tests still pass) and physically sane
    // for the BDM-800 (e.g. 3.8 A + 4.2 A = 8.0 A total).
    //
    // NOTE: the byte->channel mapping (31 = ch1, 32 = ch2) is a HYPOTHESIS
    // derived from a single BDM-800 packet — see CLAUDE.md. Treat `dc_current_a`
    // (the total) as the robust figure and confirm ch1/ch2 individually against
    // a capture series before relying on them.
    let dc_current_ch1_a = (dc_current_raw >> 8) as f64 / 10.0;
    let dc_current_ch2_a = (dc_current_raw & 0xFF) as f64 / 10.0;
    let dc_current_a = dc_current_ch1_a + dc_current_ch2_a;
    let ac_freq_hz = ac_freq_raw as f64 / 256.0;
    let temp_c = temp_raw as f64 / 100.0;
    let daily_energy_wh = daily_energy_raw as f64 / 5.0; // 0.2 Wh per unit
    let dc_voltage_v = if dc_current_a > 0.05 {
        (ac_power_w / (0.96 * dc_current_a)).min(60.0)
    } else {
        0.0
    };
    let reactive_power_var = reactive_power_raw as f64 / 100.0;
    let version = format!("{}.{:02}", version_raw / 100, version_raw % 100);

    Ok((
        input,
        NepTelemetry {
            serial_number,
            status_code,
            ac_power_w,
            ac_voltage_v,
            operating_flags,
            dc_current_a,
            dc_current_ch1_a,
            dc_current_ch2_a,
            ac_freq_hz,
            dc_voltage_v,
            temp_c,
            daily_energy_wh,
            version,
            version_raw,
            reactive_power_var,
        },
    ))
}

/// Parses the entire 45-byte payload.
/// pub fn parse_payload(input: &[u8]) -> Result<NepTelemetry, String> {
pub fn parse_payload(input: &[u8]) -> Result<NepTelemetry, String> {
    if input.len() < 45 {
        return Err(format!(
            "Payload too short: expected 45 bytes, got {}",
            input.len()
        ));
    }
    if !validate_checksums(input) {
        return Err("Checksum validation failed".to_string());
    }

    let (remaining, _) =
        parse_header(input).map_err(|e| format!("Header parsing failed: {:?}", e))?;
    let (_, telemetry) = parse_data_section(remaining)
        .map_err(|e| format!("Data section parsing failed: {:?}", e))?;
    Ok(telemetry)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_payload1() {
        let hex = "7926004014ffffffffffffffff1c00c3c3c3c3785634120000755752172010002bfe31cf145c1a0684bb2e395f";
        let bytes = hex::decode(hex).unwrap();
        assert!(validate_checksums(&bytes));
        let telemetry = parse_payload(&bytes).unwrap();
        assert_eq!(telemetry.serial_number, 0x12345678);
        assert_eq!(telemetry.ac_power_w, 223.89);
        assert_eq!(telemetry.ac_voltage_v, 233.203125); // 5970 / 25.6
        assert_eq!(telemetry.ac_freq_hz, 49.9921875); // 12798 / 256
        assert_eq!(telemetry.temp_c, 53.27);
        assert_eq!(telemetry.daily_energy_wh, 1349.6);
    }

    #[test]
    fn test_parse_payload6() {
        let hex = "7926004014ffffffffffffffff1c00c3c3c3c3785634120000443552183010001af8312f136a21068402026f5b";
        let bytes = hex::decode(hex).unwrap();
        assert!(validate_checksums(&bytes));
        let telemetry = parse_payload(&bytes).unwrap();
        assert_eq!(telemetry.serial_number, 0x12345678);
        assert_eq!(telemetry.ac_power_w, 136.36);
        assert_eq!(telemetry.dc_current_a, 2.6);
        assert_eq!(telemetry.reactive_power_var, 5.14);
        assert_eq!(telemetry.temp_c, 49.11);
        assert_eq!(telemetry.daily_energy_wh, 1710.8);
    }

    #[test]
    fn test_parse_payload_bdm800() {
        // NEP BDM-800 packet captured from a live unit. The serial number has
        // been anonymized to 0xDEADBEEF (bytes 19..22) with both checksums
        // recomputed; every other byte is as captured.
        // Distinguishing features vs the BDM-400 fixtures above:
        //   - Gateway/AP field (bytes 5..12) is `00 00 0f 0f 0f 0f 00 00`, NOT
        //     0xFF padding. The old strict `tag([0xFF; 8])` rejected this packet.
        //   - Both DC-current bytes (31, 32) are populated (dual-MPPT), so the
        //     old `be_u16 / 10` produced ~977 A. Per-channel decode fixes it.
        let hex = "792600401400000f0f0f0f00001c00c3c3c3c3efbeadde0000bd56cb172010262ab0317013ab12058c05e02270";
        let bytes = hex::decode(hex).unwrap();
        assert!(validate_checksums(&bytes));
        let t = parse_payload(&bytes).unwrap();

        // AC-side fields decode identically to the BDM-400 layout.
        assert_eq!(t.serial_number, 0xdeadbeef);
        assert_eq!(t.ac_power_w, 222.05);
        assert_eq!(t.ac_freq_hz, 49.6875); // 12720 / 256
        assert!((t.ac_voltage_v - 237.93).abs() < 0.01); // 6091 / 25.6
        assert_eq!(t.daily_energy_wh, 955.8);

        // DC-side: two per-string currents that sum to a physically sane total,
        // instead of the ~977 A the single-BE-u16 interpretation produced.
        assert_eq!(t.dc_current_ch1_a, 3.8); // byte 31 = 0x26 = 38 -> /10
        assert_eq!(t.dc_current_ch2_a, 4.2); // byte 32 = 0x2a = 42 -> /10
        assert_eq!(t.dc_current_a, 8.0);
    }

    #[test]
    fn test_parse_payload_bdm800_post_reset_sync_marker() {
        // Captured from the same live BDM-800 on the first report after the
        // inverter was reset (serial anonymized to 0xDEADBEEF, checksums
        // recomputed; every other byte as captured). Bytes 15..19 — normally
        // the 0xC3C3C3C3 sync marker — read 0xFFFFFFFF here, which the old
        // exact match rejected despite both checksums validating.
        let hex = "792600401400000f0f0f0f00001c00ffffffffefbeadde0000b8ae581650104a55a631cf149104058c94cd1a42";
        let bytes = hex::decode(hex).unwrap();
        assert!(validate_checksums(&bytes));
        let t = parse_payload(&bytes).unwrap();

        assert_eq!(t.serial_number, 0xdeadbeef);
        assert_eq!(t.ac_power_w, 447.28);
        assert!((t.ac_voltage_v - 223.44).abs() < 0.01); // 5720 / 25.6
        assert_eq!(t.ac_freq_hz, 49.6484375); // 12710 / 256
        assert_eq!(t.temp_c, 53.27);
        assert_eq!(t.daily_energy_wh, 233.8); // low: reset cleared the accumulator
        assert_eq!(t.dc_current_ch1_a, 7.4); // byte 31 = 0x4a = 74 -> /10
        assert_eq!(t.dc_current_ch2_a, 8.5); // byte 32 = 0x55 = 85 -> /10
        assert_eq!(t.reactive_power_var, -129.08);
    }

    #[test]
    fn test_parse_payload_rejects_bad_checksums() {
        // Corrupting any body byte must fail checksum validation, now the only
        // integrity guard for the relaxed gateway/AP and sync-marker fields.
        let hex = "792600401400000f0f0f0f00001c00ffffffffefbeadde0000b8ae581650104a55a631cf149104058c94cd1a42";
        let mut bytes = hex::decode(hex).unwrap();
        bytes[25] ^= 0x01;
        assert!(!validate_checksums(&bytes));
        assert_eq!(
            parse_payload(&bytes).unwrap_err(),
            "Checksum validation failed"
        );
    }
}
