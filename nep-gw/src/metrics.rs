use lazy_static::lazy_static;
use prometheus::{Gauge, IntCounter, Registry};

lazy_static! {
    pub static ref REGISTRY: Registry = Registry::new();
    pub static ref PACKETS_RECEIVED: IntCounter = IntCounter::new(
        "nep_packets_received_total",
        "Total number of valid telemetry packets received from the inverter"
    )
    .unwrap();
    pub static ref UPSTREAM_FORWARDS: IntCounter = IntCounter::new(
        "nep_upstream_forwards_total",
        "Packets successfully relayed to the real NEP cloud (dual-delivery mode)"
    )
    .unwrap();
    pub static ref UPSTREAM_FORWARD_ERRORS: IntCounter = IntCounter::new(
        "nep_upstream_forward_errors_total",
        "Packets that failed to relay to the real NEP cloud (dual-delivery mode)"
    )
    .unwrap();
    pub static ref AC_POWER: Gauge = Gauge::new(
        "nep_inverter_ac_power_watts",
        "AC output active power in Watts"
    )
    .unwrap();
    pub static ref AC_VOLTAGE: Gauge =
        Gauge::new("nep_inverter_ac_voltage_volts", "Grid AC Voltage in Volts").unwrap();
    pub static ref DC_CURRENT: Gauge = Gauge::new(
        "nep_inverter_dc_current_amperes",
        "Total DC Input Current in Amperes (sum of MPPT channels)"
    )
    .unwrap();
    pub static ref DC_CURRENT_CH1: Gauge = Gauge::new(
        "nep_inverter_dc_current_ch1_amperes",
        "DC Input Current, MPPT channel 1 in Amperes (BDM-800; always 0 on BDM-400)"
    )
    .unwrap();
    pub static ref DC_CURRENT_CH2: Gauge = Gauge::new(
        "nep_inverter_dc_current_ch2_amperes",
        "DC Input Current, MPPT channel 2 in Amperes (BDM-800; single string on BDM-400)"
    )
    .unwrap();
    pub static ref AC_FREQ: Gauge =
        Gauge::new("nep_inverter_ac_freq_hz", "Grid Frequency in Hertz").unwrap();
    pub static ref DC_VOLTAGE: Gauge = Gauge::new(
        "nep_inverter_dc_voltage_volts",
        "DC PV Input Voltage in Volts"
    )
    .unwrap();
    pub static ref TEMPERATURE: Gauge = Gauge::new(
        "nep_inverter_temperature_celsius",
        "Inverter internal temperature in Celsius"
    )
    .unwrap();
    pub static ref DAILY_ENERGY: Gauge = Gauge::new(
        "nep_inverter_daily_energy_watt_hours",
        "Daily energy accumulated by the inverter in Watt-hours"
    )
    .unwrap();
    pub static ref REACTIVE_POWER: Gauge = Gauge::new(
        "nep_inverter_reactive_power_var",
        "AC output reactive power in Volt-Amperes Reactive"
    )
    .unwrap();
}

pub fn register_metrics() {
    REGISTRY
        .register(Box::new(PACKETS_RECEIVED.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(UPSTREAM_FORWARDS.clone()))
        .unwrap();
    REGISTRY
        .register(Box::new(UPSTREAM_FORWARD_ERRORS.clone()))
        .unwrap();
    REGISTRY.register(Box::new(AC_POWER.clone())).unwrap();
    REGISTRY.register(Box::new(AC_VOLTAGE.clone())).unwrap();
    REGISTRY.register(Box::new(DC_CURRENT.clone())).unwrap();
    REGISTRY.register(Box::new(DC_CURRENT_CH1.clone())).unwrap();
    REGISTRY.register(Box::new(DC_CURRENT_CH2.clone())).unwrap();
    REGISTRY.register(Box::new(AC_FREQ.clone())).unwrap();
    REGISTRY.register(Box::new(DC_VOLTAGE.clone())).unwrap();
    REGISTRY.register(Box::new(TEMPERATURE.clone())).unwrap();
    REGISTRY.register(Box::new(DAILY_ENERGY.clone())).unwrap();
    REGISTRY.register(Box::new(REACTIVE_POWER.clone())).unwrap();
}
