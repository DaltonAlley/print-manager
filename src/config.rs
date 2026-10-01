use std::{env, fmt, net::SocketAddr, path::PathBuf};

use chrono_tz::Tz;
use thiserror::Error;

const DEFAULT_DATABASE_URL: &str = "sqlite:data/print-manager.db";
const DEFAULT_BIND_ADDR: &str = "127.0.0.1:3000";
const DEFAULT_DATA_ROOT: &str = "data";
pub const DEFAULT_SHOP_TIME_ZONE: Tz = chrono_tz::America::New_York;
pub const DEFAULT_ATTACHMENT_MAX_BYTES: u64 = 100 * 1024 * 1024;

#[derive(Clone, Eq, PartialEq)]
pub struct AppConfig {
    pub database_url: String,
    pub bind_addr: SocketAddr,
    pub resend_api_key: Option<String>,
    pub resend_from: Option<String>,
    pub data_root: PathBuf,
    pub attachment_max_bytes: u64,
    pub shop_time_zone: Tz,
}

impl fmt::Debug for AppConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AppConfig")
            .field("database_url", &self.database_url)
            .field("bind_addr", &self.bind_addr)
            .field("resend_configured", &self.resend_api_key.is_some())
            .field("data_root", &self.data_root)
            .field("attachment_max_bytes", &self.attachment_max_bytes)
            .field("shop_time_zone", &self.shop_time_zone)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Error, Eq, PartialEq)]
pub enum ConfigError {
    #[error("BIND_ADDR must be a socket address, got {value:?}")]
    InvalidBindAddress { value: String },
    #[error("RESEND_API_KEY and RESEND_FROM must either both be set or both be absent")]
    IncompleteResendConfiguration,
    #[error("ATTACHMENT_MAX_BYTES must be a positive integer, got {value:?}")]
    InvalidAttachmentMaxBytes { value: String },
    #[error("SHOP_TIME_ZONE must be a valid IANA timezone, got {value:?}")]
    InvalidShopTimeZone { value: String },
}

impl AppConfig {
    /// Reads runtime configuration from the process environment.
    ///
    /// # Errors
    ///
    /// Returns an error when the bind address is invalid or Resend is only partially configured.
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_lookup(|name| env::var(name).ok())
    }

    fn from_lookup(mut lookup: impl FnMut(&str) -> Option<String>) -> Result<Self, ConfigError> {
        let database_url =
            nonempty(lookup("DATABASE_URL")).unwrap_or_else(|| DEFAULT_DATABASE_URL.to_owned());
        let bind_value =
            nonempty(lookup("BIND_ADDR")).unwrap_or_else(|| DEFAULT_BIND_ADDR.to_owned());
        let bind_addr = bind_value
            .parse()
            .map_err(|_| ConfigError::InvalidBindAddress { value: bind_value })?;
        let resend_api_key = nonempty(lookup("RESEND_API_KEY"));
        let resend_from = nonempty(lookup("RESEND_FROM"));
        if resend_api_key.is_some() != resend_from.is_some() {
            return Err(ConfigError::IncompleteResendConfiguration);
        }
        let data_root = PathBuf::from(
            nonempty(lookup("PRINT_MANAGER_DATA_ROOT"))
                .unwrap_or_else(|| DEFAULT_DATA_ROOT.to_owned()),
        );
        let attachment_max_bytes = match nonempty(lookup("ATTACHMENT_MAX_BYTES")) {
            Some(value) => value
                .parse::<u64>()
                .ok()
                .filter(|value| *value > 0)
                .ok_or(ConfigError::InvalidAttachmentMaxBytes { value })?,
            None => DEFAULT_ATTACHMENT_MAX_BYTES,
        };
        let shop_time_zone = match nonempty(lookup("SHOP_TIME_ZONE")) {
            Some(value) => value
                .parse::<Tz>()
                .map_err(|_| ConfigError::InvalidShopTimeZone { value })?,
            None => DEFAULT_SHOP_TIME_ZONE,
        };

        Ok(Self {
            database_url,
            bind_addr,
            resend_api_key,
            resend_from,
            data_root,
            attachment_max_bytes,
            shop_time_zone,
        })
    }
}

fn nonempty(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    #[test]
    fn defaults_to_loopback_and_one_local_sqlite_file() {
        let config = AppConfig::from_lookup(|_| None).unwrap_or_else(|error| panic!("{error}"));

        assert_eq!(
            config.bind_addr,
            "127.0.0.1:3000"
                .parse()
                .unwrap_or_else(|error| panic!("{error}"))
        );
        assert_eq!(config.database_url, "sqlite:data/print-manager.db");
        assert_eq!(config.resend_api_key, None);
        assert_eq!(config.data_root, PathBuf::from("data"));
        assert_eq!(config.attachment_max_bytes, 100 * 1024 * 1024);
        assert_eq!(config.shop_time_zone, chrono_tz::America::New_York);
    }

    #[test]
    fn rejects_half_configured_email() {
        let values = HashMap::from([("RESEND_API_KEY", "secret".to_owned())]);
        let result = AppConfig::from_lookup(|name| values.get(name).cloned());

        assert_eq!(result, Err(ConfigError::IncompleteResendConfiguration));
    }

    #[test]
    fn validates_attachment_limit_and_data_root() {
        let values = HashMap::from([
            ("PRINT_MANAGER_DATA_ROOT", "/srv/print-data".to_owned()),
            ("ATTACHMENT_MAX_BYTES", "4096".to_owned()),
        ]);
        let config = AppConfig::from_lookup(|name| values.get(name).cloned())
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(config.data_root, PathBuf::from("/srv/print-data"));
        assert_eq!(config.attachment_max_bytes, 4096);

        let invalid = HashMap::from([("ATTACHMENT_MAX_BYTES", "0".to_owned())]);
        assert!(matches!(
            AppConfig::from_lookup(|name| invalid.get(name).cloned()),
            Err(ConfigError::InvalidAttachmentMaxBytes { .. })
        ));
    }

    #[test]
    fn validates_shop_time_zone() {
        let values = HashMap::from([("SHOP_TIME_ZONE", "America/Chicago".to_owned())]);
        let config = AppConfig::from_lookup(|name| values.get(name).cloned())
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(config.shop_time_zone, chrono_tz::America::Chicago);

        let invalid = HashMap::from([("SHOP_TIME_ZONE", "Eastern".to_owned())]);
        assert!(matches!(
            AppConfig::from_lookup(|name| invalid.get(name).cloned()),
            Err(ConfigError::InvalidShopTimeZone { .. })
        ));
    }
}
