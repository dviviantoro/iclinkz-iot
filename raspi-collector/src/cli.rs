use clap::{Parser, ValueEnum};

#[derive(Parser, Debug)]
#[command(
    name    = "raspi-collector",
    about   = "Raspberry Pi IoT sensor collector — RS485 water quality, USB pump monitor, AHT10 ambient",
    version
)]
pub struct Cli {
    /// Operating mode
    #[arg(short, long, default_value = "mqtt", value_enum)]
    pub mode: Mode,

    /// Path to .env file (default: .env in current working directory)
    #[arg(short, long, default_value = ".env")]
    pub env_file: std::path::PathBuf,
}

#[derive(Clone, Debug, ValueEnum)]
pub enum Mode {
    /// Read sensors and log to stdout only — no MQTT connection
    ReadOnly,
    /// Read sensors and publish data to MQTT broker (default)
    Mqtt,
    /// Publish sensor data and subscribe to incoming control commands
    Subscriber,
}
