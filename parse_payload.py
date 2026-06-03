import struct

def parse_nep_payload(hex_str, server_time_str=""):
    data = bytes.fromhex(hex_str)
    
    # Header fields (15 bytes)
    signature = data[0]
    length_field = struct.unpack("<H", data[1:3])[0]
    cmd_type = data[3:5].hex()
    gateway_id = data[5:13].hex()
    data_length = struct.unpack("<H", data[13:15])[0]
    
    # Checksums (2 bytes)
    calc_sum = sum(data[1:43]) & 0xff
    calc_xor = 0
    for b in data[1:43]:
        calc_xor ^= b
        
    received_sum = data[43]
    received_xor = data[44]
    
    # Data fields (28 bytes, non-overlapping)
    sync_marker = data[15:19].hex()
    serial_number = struct.unpack("<I", data[19:23])[0]
    status_code = struct.unpack("<H", data[23:25])[0]
    ac_power_raw = struct.unpack("<H", data[25:27])[0]     # LE, centi-Watts (divided by 100)
    ac_voltage_raw = struct.unpack("<H", data[27:29])[0]   # LE, Q8 decivolts (divided by 25.6)
    operating_flags = struct.unpack("<H", data[29:31])[0]  # LE, status flags
    dc_current_raw = struct.unpack(">H", data[31:33])[0]   # BE, deciamperes (divided by 10)
    ac_freq_raw = struct.unpack("<H", data[33:35])[0]       # LE, Q8 Hz (divided by 256)
    temp_raw = struct.unpack("<H", data[35:37])[0]          # LE, centidegrees Celsius (divided by 100)
    daily_energy_raw = struct.unpack("<H", data[37:39])[0]  # LE, accumulated Wh / 5 (scaled by 0.2 Wh)
    version_raw = struct.unpack("<H", data[39:41])[0]       # LE, BCD version
    reactive_power_raw = struct.unpack("<h", data[41:43])[0] # LE, signed centi-VAR
    
    # Conversions
    serial_hex = f"{serial_number:08x}"
    ac_power_w = ac_power_raw / 100.0
    ac_voltage = ac_voltage_raw / 25.6
    dc_current_a = dc_current_raw / 10.0
    ac_freq_hz = ac_freq_raw / 256.0
    temp_c = temp_raw / 100.0
    daily_energy_wh = daily_energy_raw / 5.0
    reactive_power_var = reactive_power_raw / 100.0
    version_str = f"{version_raw // 100}.{version_raw % 100:02d}"
    
    # Calculated DC PV Power & Voltage (since DC voltage isn't natively sent in the payload)
    dc_voltage = min(ac_power_w / (0.96 * dc_current_a), 60.0) if dc_current_a > 0.05 else 0.0
    dc_power_w = dc_voltage * dc_current_a
    efficiency = (ac_power_w / dc_power_w * 100.0) if dc_power_w > 0 else 0
    
    return {
        "time": server_time_str,
        "serial": serial_hex,
        "status": status_code,
        "ac_power": ac_power_w,
        "ac_voltage": ac_voltage,
        "ac_freq": ac_freq_hz,
        "dc_voltage": dc_voltage,
        "dc_current": dc_current_a,
        "dc_power": dc_power_w,
        "efficiency": efficiency,
        "temp": temp_c,
        "daily_energy": daily_energy_wh,
        "operating_flags": f"0x{operating_flags:04x}",
        "reactive_power": reactive_power_var,
        "version": version_str,
        "checksum_ok": (received_sum == calc_sum) and (received_xor == calc_xor)
    }

payloads = [
    ("2026-05-30 15:43:41", "79 26 00 40 14 ff ff ff ff ff ff ff ff 1c 00 c3 c3 c3 c3 78 56 34 12 00 00 75 57 52 17 20 10 00 2b fe 31 cf 14 5c 1a 06 84 bb 2e 39 5f"),
    ("2026-05-30 15:44:41", "79 26 00 40 14 ff ff ff ff ff ff ff ff 1c 00 c3 c3 c3 c3 78 56 34 12 00 00 c5 50 76 17 30 10 00 28 fe 31 bf 14 70 1a 06 84 43 25 36 70"),
    ("2026-05-30 16:06:28", "79 26 00 40 14 ff ff ff ff ff ff ff ff 1c 00 c3 c3 c3 c3 78 56 34 12 00 00 47 2d cb 17 00 10 00 16 08 32 ff 13 e8 1b 06 84 ac f0 9f 2d"),
    ("2026-05-30 17:30:39", "79 26 00 40 14 ff ff ff ff ff ff ff ff 1c 00 c3 c3 c3 c3 78 56 34 12 00 00 37 35 f6 17 30 10 00 1a 04 32 90 13 ea 20 06 84 86 01 75 c5"),
    ("2026-05-30 17:35:36", "79 26 00 40 14 ff ff ff ff ff ff ff ff 1c 00 c3 c3 c3 c3 78 56 34 12 00 00 d8 37 12 18 20 10 00 1b 04 32 50 13 2a 21 06 84 33 05 d8 62"),
    ("2026-05-30 17:40:33", "79 26 00 40 14 ff ff ff ff ff ff ff ff 1c 00 c3 c3 c3 c3 78 56 34 12 00 00 44 35 52 18 30 10 00 1a f8 31 2f 13 6a 21 06 84 02 02 6f 5b"),
    # Payloads from pv (5).pcap
    ("2026-05-30 18:25:07", "79 26 00 40 14 ff ff ff ff ff ff ff ff 1c 00 c3 c3 c3 c3 78 56 34 12 00 00 c9 01 0d 16 30 10 00 01 fc 31 8f 11 a5 22 06 84 e1 b2 8d 91"),
    ("2026-05-30 18:30:04", "79 26 00 40 14 ff ff ff ff ff ff ff ff 1c 00 c3 c3 c3 c3 78 56 34 12 00 00 00 00 5f 1a 30 10 00 00 02 32 50 11 a7 22 01 84 2e b4 2c e8"),
    ("2026-05-30 18:35:01", "79 26 00 40 14 ff ff ff ff ff ff ff ff 1c 00 c3 c3 c3 c3 78 56 34 12 00 00 00 00 4f 1a 30 10 00 00 04 32 10 11 a7 22 01 84 e0 b3 8f 77"),
    ("2026-05-30 18:39:58", "79 26 00 40 14 ff ff ff ff ff ff ff ff 1c 00 c3 c3 c3 c3 78 56 34 12 00 00 00 00 4f 1a 30 10 00 00 fe 31 df 10 a7 22 01 84 a9 b3 1f 09"),
    ("2026-05-30 18:44:56", "79 26 00 40 14 ff ff ff ff ff ff ff ff 1c 00 c3 c3 c3 c3 78 56 34 12 00 00 00 00 6f 1a 30 10 00 00 04 32 b0 10 a7 22 01 84 a0 b3 0e b6"),
    ("2026-05-30 18:49:53", "79 26 00 40 14 ff ff ff ff ff ff ff ff 1c 00 c3 c3 c3 c3 78 56 34 12 00 00 75 02 bb 14 30 10 00 01 06 32 90 10 a8 22 06 84 49 b1 5b db"),
    ("2026-05-30 18:54:50", "79 26 00 40 14 ff ff ff ff ff ff ff ff 1c 00 c3 c3 c3 c3 78 56 34 12 00 00 41 02 48 14 30 10 00 01 fe 31 80 10 ac 22 06 84 8e b0 e3 35"),
    ("2026-05-30 18:59:47", "79 26 00 40 14 ff ff ff ff ff ff ff ff 1c 00 c3 c3 c3 c3 78 56 34 12 00 00 2b 03 f8 15 00 10 00 02 06 32 7d 10 af 22 06 84 00 b4 cf 53"),
    ("2026-05-30 19:04:44", "79 26 00 40 14 ff ff ff ff ff ff ff ff 1c 00 c3 c3 c3 c3 78 56 34 12 00 00 bc 06 f4 15 f0 0f 00 03 fc 31 70 10 b6 22 06 84 6d b8 af af"),
    ("2026-05-30 19:09:42", "79 26 00 40 14 ff ff ff ff ff ff ff ff 1c 00 c3 c3 c3 c3 78 56 34 12 00 00 f1 02 da 15 e0 0f 00 02 fa 31 70 10 bc 22 06 84 7c b3 c3 cf"),
    ("2026-05-30 19:14:38", "79 26 00 40 14 ff ff ff ff ff ff ff ff 1c 00 c3 c3 c3 c3 78 56 34 12 00 00 00 00 80 1a f0 0f 00 00 04 32 70 10 be 22 04 84 4b b3 63 b1"),
    ("2026-05-30 19:19:35", "79 26 00 40 14 ff ff ff ff ff ff ff ff 1c 00 c3 c3 c3 c3 78 56 34 12 00 00 00 00 ef 19 00 10 00 00 06 32 50 10 bf 22 01 84 aa b2 20 f4"),
    # Payloads from pv (6).pcap
    ("2026-05-31 08:04:01", "79 26 00 40 14 ff ff ff ff ff ff ff ff 1c 00 c3 c3 c3 c3 78 56 34 12 00 00 af 09 7e 15 f0 0f 00 04 f8 31 d1 0d 62 00 06 84 f3 96 78 c0"),
    ("2026-05-31 08:05:00", "79 26 00 40 14 ff ff ff ff ff ff ff ff 1c 00 c3 c3 c3 c3 78 56 34 12 00 00 dd 09 68 15 f0 0f 00 04 fc 31 d1 0d 64 00 06 84 11 97 b5 45"),
    ("2026-05-31 08:09:57", "79 26 00 40 14 ff ff ff ff ff ff ff ff 1c 00 c3 c3 c3 c3 78 56 34 12 00 00 5f 0a 62 15 f0 0f 00 06 04 32 e0 0d 70 00 06 84 b0 99 f9 bd"),
    # Payloads from June 1st grid event (pod logs)
    ("2026-06-01 05:50:57", "79 26 00 40 14 ff ff ff ff ff ff ff ff 1c 00 c3 c3 c3 c3 78 56 34 12 00 00 00 00 df 17 00 10 00 00 00 32 e0 0c 00 00 01 84 65 8a 46 0a"),
    ("2026-06-01 05:51:57", "79 26 00 40 14 ff ff ff ff ff ff ff ff 1c 00 c3 c3 c3 c3 78 56 34 12 00 00 00 00 3f 18 00 10 00 00 fa 31 e0 0c 00 00 01 84 bf 8a fa c6"),
    ("2026-06-01 05:52:56", "79 26 00 40 14 ff ff ff ff ff ff ff ff 1c 00 c3 c3 c3 c3 78 56 34 12 00 00 01 00 8b 0f 00 10 00 01 fe 31 e0 0c 00 00 05 84 14 83 95 c7"),
    ("2026-06-01 05:53:56", "79 26 00 40 14 ff ff ff ff ff ff ff ff 1c 00 c3 c3 c3 c3 78 56 34 12 00 00 00 00 bf 18 00 10 00 00 fc 31 f0 0c 00 00 01 84 51 8b 1f bf"),
    ("2026-06-01 05:54:52", "79 26 00 40 14 ff ff ff ff ff ff ff ff 1c 00 c3 c3 c3 c3 78 56 34 12 20 00 00 00 ef 18 00 10 00 00 f6 31 00 0d 00 00 01 84 ab 8b d4 ce"),
    ("2026-06-01 05:55:51", "79 26 00 40 14 ff ff ff ff ff ff ff ff 1c 00 c3 c3 c3 c3 78 56 34 12 20 00 00 00 0f 19 10 10 00 00 fa 31 00 0d 00 00 01 84 df 8b 3d 47"),
    ("2026-06-01 05:56:51", "79 26 00 40 14 ff ff ff ff ff ff ff ff 1c 00 c3 c3 c3 c3 78 56 34 12 04 00 00 00 f4 12 00 10 00 00 fc 31 f0 0c 00 00 04 84 8d 85 8b 2d"),
    ("2026-06-01 05:57:50", "79 26 00 40 14 ff ff ff ff ff ff ff ff 1c 00 c3 c3 c3 c3 78 56 34 12 00 00 00 00 4f 19 00 10 00 00 fc 31 f0 0c 00 00 01 84 e1 8b 40 fe"),
    ("2026-06-01 05:58:50", "79 26 00 40 14 ff ff ff ff ff ff ff ff 1c 00 c3 c3 c3 c3 78 56 34 12 00 00 00 00 7f 19 00 10 00 00 fe 31 f0 0c 00 00 01 84 13 8c a5 39"),
    ("2026-06-01 05:59:49", "79 26 00 40 14 ff ff ff ff ff ff ff ff 1c 00 c3 c3 c3 c3 78 56 34 12 00 00 00 00 bf 19 00 10 00 00 06 32 f1 0c 00 00 01 84 5c 8c 38 4c")
]

if __name__ == "__main__":
    parsed = [parse_nep_payload(hex_str, t) for t, hex_str in payloads]
    
    # Print detailed view for the latest payload
    latest = parsed[-1]
    print("==================================================")
    print("             LATEST TELEMETRY DECODED             ")
    print("==================================================")
    print(f"Time: {latest['time']}")
    print(f"Inverter Serial Number: {latest['serial']}")
    print(f"AC Output Power: {latest['ac_power']:.2f} W")
    print(f"AC Grid Voltage: {latest['ac_voltage']:.2f} V")
    print(f"AC Grid Frequency: {latest['ac_freq']:.2f} Hz")
    print(f"DC PV Voltage: {latest['dc_voltage']:.2f} V (Estimated)")
    print(f"DC PV Current: {latest['dc_current']:.2f} A")
    print(f"DC PV Power: {latest['dc_power']:.2f} W (Estimated)")
    print(f"Inverter Efficiency: {latest['efficiency']:.1f}%")
    print(f"Inverter Temperature: {latest['temp']:.2f} °C")
    print(f"Daily Energy: {latest['daily_energy']:.2f} Wh")
    print(f"Operating Flags: {latest['operating_flags']}")
    print(f"Reactive Power: {latest['reactive_power']:.2f} VAR")
    print(f"Firmware Version: {latest['version']}")
    print(f"Checksums Verified: {latest['checksum_ok']}")
    print("==================================================")

    # Print comparison table
    print("\n" + "="*124)
    print("                                              CHRONOLOGICAL TELEMETRY TABLE                                             ")
    print("="*124)
    print("| Time                | AC Power | AC Volt | AC Freq | DC Volt | DC Curr | DC Power | Temp  | Daily Energy | Op Flags | Reactive  |")
    print("| :------------------ | :------- | :------ | :------ | :------ | :------ | :------- | :---- | :----------- | :------- | :-------- |")
    for p in parsed:
        print(f"| {p['time']} | {p['ac_power']:7.2f}W | {p['ac_voltage']:6.2f}V | {p['ac_freq']:6.2f}Hz | {p['dc_voltage']:6.2f}V | {p['dc_current']:6.2f}A | {p['dc_power']:7.2f}W | {p['temp']:4.1f}°C | {p['daily_energy']:9.1f}Wh | {p['operating_flags']:8s} | {p['reactive_power']:8.2f}V |")
    print("="*124)
    print("W = Watts, V = Volts, Hz = Hertz, A = Amperes, VAR = Volt-Amperes Reactive")
