use std::{error::Error, net::SocketAddr};

use clap::Parser;
use serde::Deserialize;
use tokio::fs::read_to_string;

const CONFIG_PATH: &str = "/etc/dwitch/config.toml";

pub type SwitchId = u32;

#[derive(Debug, Clone, Deserialize, Parser)]
pub struct Config {
    pub switch_id: SwitchId,
    pub listen: SocketAddr,
    pub servers: Vec<SocketAddr>,
}

impl Config {
    pub async fn load() -> Result<Config, Box<dyn Error>> {
        Ok(toml::from_str(&read_to_string(CONFIG_PATH).await?)?)
    }
}
