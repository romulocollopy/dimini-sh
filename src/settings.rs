use config::{Config, Environment, File, FileFormat};
use std::net::SocketAddr;

#[derive(Debug, PartialEq, serde::Deserialize)]
pub enum Env {
    Test,
    Prod,
}

#[derive(Debug, serde::Deserialize)]
pub struct Settings {
    database_url: String,
    env: Env,
    host: String,
    port: u16,
    short_code_length: u8,
}

impl Settings {
    /// Load production settings from `settings.yaml` and environment variables.
    pub fn load() -> Self {
        Config::builder()
            .add_source(File::new("settings", FileFormat::Yaml))
            .add_source(Environment::default())
            .set_override("env", "Prod")
            .unwrap()
            .build()
            .unwrap()
            .try_deserialize()
            .unwrap()
    }

    /// Return a hardcoded test configuration.
    pub fn testing() -> Self {
        Settings {
            database_url: "postgres://dev:somepass@postgres:5432/diminish_test".to_string(),
            env: Env::Test,
            host: "0.0.0.0".to_string(),
            port: 3000,
            short_code_length: 4,
        }
    }

    /// Return the configured short code length.
    pub fn get_short_code_length(&self) -> u8 {
        self.short_code_length
    }

    /// Return the database URL, panicking if in Test env but URL lacks "test".
    pub fn get_database_url(&self) -> &str {
        if self.env == Env::Test && !self.database_url.contains("test") {
            panic!("Test env requires a database URL containing 'test'");
        }
        &self.database_url
    }

    /// Parse `host:port` into a `SocketAddr`.
    pub fn get_host(&self) -> SocketAddr {
        format!("{}:{}", self.host, self.port)
            .parse::<SocketAddr>()
            .unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `Settings::load()` must return prod settings with the expected database_url and Env::Prod.
    #[test]
    fn load_returns_prod_settings() {
        let settings = Settings::load();
        assert_eq!(settings.env, Env::Prod);
        assert_eq!(
            settings.database_url,
            "postgres://dev:somepass@postgres:5432/diminish"
        );
    }

    /// `Settings::testing()` must return test settings with the test database URL and Env::Test.
    #[test]
    fn testing_returns_test_settings() {
        let settings = Settings::testing();
        assert_eq!(settings.env, Env::Test);
        assert_eq!(
            settings.database_url,
            "postgres://dev:somepass@postgres:5432/diminish_test"
        );
    }

    /// `get_database_url()` must panic when env is Test but the URL does not contain "test".
    #[test]
    #[should_panic]
    fn get_database_url_panics_when_test_env_has_no_test_in_url() {
        let settings = Settings {
            database_url: "postgres://dev:somepass@postgres:5432/diminish".to_string(),
            env: Env::Test,
            host: "0.0.0.0".to_string(),
            port: 3000,
            short_code_length: 4,
        };
        settings.get_database_url();
    }

    /// `Settings::testing()` must expose `short_code_length` of 4.
    ///
    /// Business rule: the default short code length is 4 characters. Tests
    /// rely on `Settings::testing()` returning a fully populated struct;
    /// `short_code_length` must be present and set to 4.
    #[test]
    fn testing_returns_short_code_length_of_4() {
        let settings = Settings::testing();
        assert_eq!(settings.get_short_code_length(), 4);
    }

    /// `get_short_code_length()` must return the value from `settings.yaml`.
    ///
    /// Business rule: `short_code_length` drives code generation at runtime.
    /// `Settings::load()` must deserialise it from the YAML config file.
    #[test]
    fn load_returns_short_code_length_of_4() {
        let settings = Settings::load();
        assert_eq!(settings.get_short_code_length(), 4);
    }

    /// `get_database_url()` must return the URL when env is Test and URL contains "test".
    #[test]
    fn get_database_url_returns_url_when_test_in_name() {
        let settings = Settings {
            database_url: "postgres://dev:somepass@postgres:5432/diminish_test".to_string(),
            env: Env::Test,
            host: "0.0.0.0".to_string(),
            port: 3000,
            short_code_length: 4,
        };
        let url = settings.get_database_url();
        assert!(url.contains("test"));
    }
}
