use lazy_static::lazy_static;
use prometheus::{Gauge, IntCounter, Registry};

lazy_static! {
    pub static ref REGISTRY: Registry = Registry::new();
    pub static ref PACKETS_RECEIVED: IntCounter = IntCounter::new(
        "nep_packets_received_total",
        "Total number of valid telemetry packets received from the inverter"
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
        "DC Input Current in Amperes"
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
    REGISTRY.register(Box::new(AC_POWER.clone())).unwrap();
    REGISTRY.register(Box::new(AC_VOLTAGE.clone())).unwrap();
    REGISTRY.register(Box::new(DC_CURRENT.clone())).unwrap();
    REGISTRY.register(Box::new(AC_FREQ.clone())).unwrap();
    REGISTRY.register(Box::new(DC_VOLTAGE.clone())).unwrap();
    REGISTRY.register(Box::new(TEMPERATURE.clone())).unwrap();
    REGISTRY.register(Box::new(DAILY_ENERGY.clone())).unwrap();
    REGISTRY.register(Box::new(REACTIVE_POWER.clone())).unwrap();
}
