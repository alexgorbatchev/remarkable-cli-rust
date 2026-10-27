use crate::error::{Error, Result};
use remarkable_cloud::{Client, ClientOptions, Endpoints};
use std::path::PathBuf;

#[derive(Clone, Debug, Default)]
pub struct Options {
    pub config: String,
    pub cache_dir: String,
    pub debug: bool,
    pub no_cache: bool,
}
pub fn config_path(override_path: &str) -> PathBuf {
    if !override_path.is_empty() {
        return override_path.into();
    }
    if let Some(value) = std::env::var_os("REMARKABLE_CONFIG").filter(|s| !s.is_empty()) {
        return value.into();
    }
    let home = dirs::home_dir();
    let xdg = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .or_else(|| home.as_ref().map(|h| h.join(".config")));
    if let Some(path) = xdg
        .map(|p| p.join("remarkable-cli/config.json"))
        .filter(|p| p.exists())
    {
        return path;
    }
    home.map_or_else(|| PathBuf::from(".rmapi"), |home| home.join(".rmapi"))
}
pub fn cache_dir(override_path: &str) -> PathBuf {
    if !override_path.is_empty() {
        return override_path.into();
    }
    if let Some(value) = std::env::var_os("REMARKABLE_CACHE_DIR").filter(|s| !s.is_empty()) {
        return value.into();
    }
    std::env::var_os("XDG_CACHE_HOME")
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|h| h.join(".cache")))
        .unwrap_or_else(|| ".cache".into())
        .join("remarkable-cli")
}
impl Options {
    pub fn client(&self, pairing: bool) -> Result<Client> {
        let path = config_path(&self.config);
        if !pairing {
            match std::fs::metadata(&path) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    return Err(Error::Credentials(cli_command_kit::display_path(&path)));
                }
                Err(error) => return Err(error.into()),
                Ok(_) => {}
            }
        }
        let mut options = ClientOptions {
            config_path: (!pairing).then_some(path),
            cache_dir: (!self.no_cache && !pairing).then(|| cache_dir(&self.cache_dir)),
            ..ClientOptions::default()
        };
        if let Ok(host) = std::env::var("REMARKABLE_HOST")
            && !host.is_empty()
        {
            options.endpoints = Endpoints {
                raw_host: host.clone(),
                webapp_host: host.clone(),
                storage_host: host.clone(),
            };
            options.auth_base_url = host;
        }
        Ok(Client::new(options)?)
    }
}
