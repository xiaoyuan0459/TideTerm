//! Serial port configuration — connection parameters for serial sessions.

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

fn default_baud_rate() -> u32 {
    115_200
}
fn default_data_bits() -> u8 {
    8
}
fn default_stop_bits() -> u8 {
    1
}
fn default_parity() -> String {
    "none".into()
}
fn default_flow_control() -> String {
    "none".into()
}

/// Serial port connection parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SerialConfig {
    pub path: String,
    #[serde(default = "default_baud_rate")]
    pub baud_rate: u32,
    #[serde(default = "default_data_bits")]
    pub data_bits: u8,
    #[serde(default = "default_stop_bits")]
    pub stop_bits: u8,
    #[serde(default = "default_parity")]
    pub parity: String,
    #[serde(default = "default_flow_control")]
    pub flow_control: String,
}

impl SerialConfig {
    pub fn validate(&self) -> Result<()> {
        if self.path.trim().is_empty() {
            return Err(Error::Serial("path is required".into()));
        }
        if self.baud_rate == 0 {
            return Err(Error::Serial("baud_rate must be greater than 0".into()));
        }
        if !matches!(self.data_bits, 5..=8) {
            return Err(Error::Serial("data_bits must be 5, 6, 7, or 8".into()));
        }
        if !matches!(self.stop_bits, 1 | 2) {
            return Err(Error::Serial("stop_bits must be 1 or 2".into()));
        }
        if !matches!(self.parity.as_str(), "none" | "odd" | "even") {
            return Err(Error::Serial("parity must be none, odd, or even".into()));
        }
        if !matches!(
            self.flow_control.as_str(),
            "none" | "hardware" | "rtscts" | "xonxoff" | "software"
        ) {
            return Err(Error::Serial(
                "flow_control must be none, hardware, rtscts, xonxoff, or software".into(),
            ));
        }
        Ok(())
    }
}

/// Information about an available serial port.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SerialPortInfo {
    pub path: String,
    pub name: Option<String>,
    pub port_type: String,
}

// ── Persisted session config (v0.5) ────────────────────────────
// De-shelled into `tiderm_core::config` (B2); re-exported here so
// `crate::protocol::serial::SerialConnectionConfig` call-sites keep working.
pub use crate::config::SerialConnectionConfig;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serial_config_round_trip() {
        let c = SerialConfig {
            path: "COM3".into(),
            baud_rate: 115_200,
            data_bits: 8,
            stop_bits: 1,
            parity: "none".into(),
            flow_control: "none".into(),
        };
        let j = serde_json::to_string(&c).expect("serialize");
        let d: SerialConfig = serde_json::from_str(&j).expect("deserialize");
        assert_eq!(d.path, "COM3");
        assert_eq!(d.baud_rate, 115_200);
    }

    #[test]
    fn serial_config_defaults() {
        let j = r#"{"path":"/dev/ttyUSB0"}"#;
        let c: SerialConfig = serde_json::from_str(j).expect("deserialize");
        assert_eq!(c.baud_rate, 115_200);
        assert_eq!(c.data_bits, 8);
        assert_eq!(c.parity, "none");
    }

    #[test]
    fn serial_config_rejects_empty_path() {
        let c = SerialConfig {
            path: "".into(),
            baud_rate: 115_200,
            data_bits: 8,
            stop_bits: 1,
            parity: "none".into(),
            flow_control: "none".into(),
        };
        assert!(c.validate().is_err());
    }

    #[test]
    fn serial_config_rejects_invalid_data_bits() {
        let c = SerialConfig {
            path: "COM3".into(),
            baud_rate: 115_200,
            data_bits: 9,
            stop_bits: 1,
            parity: "none".into(),
            flow_control: "none".into(),
        };
        assert!(c.validate().is_err());
    }

    #[test]
    fn serial_config_rejects_invalid_parity() {
        let c = SerialConfig {
            path: "COM3".into(),
            baud_rate: 115_200,
            data_bits: 8,
            stop_bits: 1,
            parity: "mark".into(),
            flow_control: "none".into(),
        };
        assert!(c.validate().is_err());
    }

    #[test]
    fn serial_port_info_serializes() {
        let i = SerialPortInfo {
            path: "COM1".into(),
            name: Some("USB Serial".into()),
            port_type: "usb".into(),
        };
        let j = serde_json::to_string(&i).expect("serialize");
        assert!(j.contains("COM1"));
    }
}
