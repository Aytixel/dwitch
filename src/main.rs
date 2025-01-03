use std::{
    collections::HashMap, error::Error, fs::read_to_string, io, str::FromStr, sync::Arc,
    time::Duration,
};

use cache::Cache;
use clap::Parser;
use config::Config;
use socket::{client::client, server::server};
use tap::initiate_tap_table;
use tappers::MacAddr;
use tokio::{sync::RwLock, task::spawn, time::sleep};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

mod cache;
mod config;
mod packet;
mod socket;
mod tap;

const BROADCAST_MAC: [u8; 6] = [0u8; 6];
const MAX_BUFFER_SIZE: usize = 65535;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    tracing_subscriber::registry()
        .with(EnvFilter::from_env("DWITCH_LOG"))
        .with(tracing_subscriber::fmt::layer())
        .init();

    // let config = Config::load()?;

    // tracing::info!("{config:#?}");

    let config = Config::parse();

    tracing::info!("{config:#?}");

    let cache = Cache::load().await.unwrap_or_default();
    let client_table = Arc::new(RwLock::new(HashMap::new()));
    let switch_table = Arc::new(RwLock::new(cache.switch_table));
    let tap_table = Arc::new(RwLock::new(initiate_tap_table(
        &cache.vrf_table,
        client_table.clone(),
        switch_table.clone(),
    )));
    let vrf_table = Arc::new(RwLock::new(cache.vrf_table));

    spawn({
        let config = config.clone();
        let tap_table = tap_table.clone();
        let vrf_table = vrf_table.clone();
        let client_table = client_table.clone();
        let switch_table = switch_table.clone();

        async {
            if let Err(error) =
                server(config, tap_table, vrf_table, client_table, switch_table).await
            {
                tracing::error!("Can't start server: {error}");
            }
        }
    });

    for address in config.servers.clone() {
        spawn(client(config.switch_id, address, client_table.clone()));
    }

    loop {
        sleep(Duration::from_secs(10)).await;

        {
            let switch_table = switch_table.read().await;
            let vrf_table = vrf_table.read().await;

            if let Err(error) = (Cache {
                switch_table: switch_table.clone(),
                vrf_table: vrf_table.clone(),
            })
            .save()
            .await
            {
                tracing::error!("Can't save cache: {error}");
            }
        }
    }
}

fn get_interface_mac(interface_name: &str) -> io::Result<MacAddr> {
    MacAddr::from_str(
        read_to_string(format!("/sys/class/net/{interface_name}/address"))?.trim_end(),
    )
    .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.as_str()))
}

trait BufferExt {
    fn clear(&mut self);
}

impl BufferExt for [u8] {
    fn clear(&mut self) {
        self.iter_mut().for_each(|byte| *byte = 0)
    }
}
