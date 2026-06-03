use nom::{
    bytes::complete::tag,
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
    pub dc_current_a: f64,
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
    // 5-12: Gateway ID (8 bytes of 0xFF)
    let (input, _) = tag([0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF])(input)?;
    // 13-14: Data Length (0x001c = 28 bytes)
    let (input, _) = tag([0x1C, 0x00])(input)?;
    Ok((input, ()))
}

fn parse_data_section(input: &[u8]) -> IResult<&[u8], NepTelemetry> {
    // 15-18: Sync marker
    let (input, _) = tag([0xC3, 0xC3, 0xC3, 0xC3])(input)?;
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
    let dc_current_a = dc_current_raw as f64 / 10.0;
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
}
