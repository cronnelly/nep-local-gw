#!/usr/bin/env python3
import os
import re
import subprocess
import json
import csv
from parse_payload import parse_nep_payload

def extract_and_save():
    print("🚀 Querying Kubernetes pod logs...")
    try:
        # Get the logs from the active pod
        pod_name = "nep-gateway-6789898699-b5mk9"
        namespace = "apps"
        result = subprocess.run(
            ["kubectl", "logs", pod_name, "-n", namespace],
            capture_output=True,
            text=True,
            check=True
        )
        log_content = result.stdout
    except Exception as e:
        print(f"❌ Failed to fetch logs from Kubernetes: {e}")
        return

    # Create the data directory if it does not exist
    data_dir = "data"
    os.makedirs(data_dir, exist_ok=True)

    # Regex to clean ANSI terminal escape sequences
    ansi_escape = re.compile(r"\x1B(?:[@-Z\\-_]|\[[0-?]*[ -/]*[@-~])")

    # Regex to parse the timestamp and raw hex payload
    pattern = re.compile(
        r"(\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d+Z)\s+.*Raw hex payload:\s+([a-fA-F0-9]+)"
    )

    records = []
    print("🔍 Extracting and decoding payloads...")
    
    for line in log_content.splitlines():
        clean_line = ansi_escape.sub("", line)
        match = pattern.search(clean_line)
        if match:
            timestamp, hex_str = match.groups()
            try:
                parsed = parse_nep_payload(hex_str, timestamp)
                # Store raw hex alongside the parsed dict
                parsed["raw_hex"] = hex_str
                records.append(parsed)
            except Exception as parse_err:
                print(f"⚠️ Failed to parse hex {hex_str} at {timestamp}: {parse_err}")

    if not records:
        print("ℹ️ No payloads found in logs.")
        return

    print(f"✅ Successfully extracted {len(records)} payloads.")

    # File paths
    jsonl_path = os.path.join(data_dir, "payloads_2026_06_01.jsonl")
    csv_path = os.path.join(data_dir, "payloads_2026_06_01.csv")

    # 1. Save to JSONL
    with open(jsonl_path, "w") as f:
        for r in records:
            f.write(json.dumps(r) + "\n")
    print(f"💾 Saved JSONL format to: {jsonl_path}")

    # 2. Save to CSV
    fieldnames = [
        "time", "serial", "status", "ac_power", "ac_voltage", "ac_freq", 
        "dc_voltage", "dc_current", "dc_power", "efficiency", "temp", 
        "daily_energy", "operating_flags", "reactive_power", "version", 
        "checksum_ok", "raw_hex"
    ]

    with open(csv_path, "w", newline="") as f:
        writer = csv.DictWriter(f, fieldnames=fieldnames)
        writer.writeheader()
        for r in records:
            row = {k: r.get(k) for k in fieldnames}
            writer.writerow(row)
    print(f"💾 Saved CSV format to: {csv_path}")

if __name__ == "__main__":
    extract_and_save()
