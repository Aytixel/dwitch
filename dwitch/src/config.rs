use std::net::SocketAddr;

use serde::Deserialize;
use tokio::fs::read_to_string;

const CONFIG_PATH: &str = "/etc/dwitch/config.toml";

pub type SwitchId = u32;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub switch_id: SwitchId,
    pub listen: SocketAddr,
    pub servers: Vec<SocketAddr>,
}

impl Config {
    pub async fn load() -> eyre::Result<Config> {
        Ok(toml::from_str(&read_to_string(CONFIG_PATH).await?)?)
    }
}
