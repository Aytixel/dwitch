use std::{collections::HashMap, sync::Arc, time::Duration};

use cache::Cache;
use config::Config;
use protocol::CONFIGURATION_SWITCH_ID;
use socket::{client::client, server::server};
use tap::initiate_tap_table;
use tokio::{sync::RwLock, task::spawn, time::sleep};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

mod cache;
mod config;
mod socket;
mod tap;

const MAX_BUFFER_SIZE: usize = 65535;

#[tokio::main]
async fn main() -> eyre::Result<()> {
    color_eyre::install()?;

    #[cfg(feature = "tokio-console")]
    console_subscriber::init();
    #[cfg(not(feature = "tokio-console"))]
    tracing_subscriber::registry()
        .with(EnvFilter::from_env("DWITCH_LOG"))
        .with(tracing_subscriber::fmt::layer())
        .init();

    let config = Config::load().await?;

    tracing::info!("{config:#?}");

    if config.switch_id == CONFIGURATION_SWITCH_ID {
        tracing::error!("Switch id can't be {CONFIGURATION_SWITCH_ID}");
        return Ok(());
    }

    let cache = Cache::load().await.unwrap_or_default();
    let client_table = Arc::new(RwLock::new(HashMap::new()));
    let switch_table = Arc::new(RwLock::new(cache.switch_table));
    let tap_table = Arc::new(RwLock::new(initiate_tap_table(
        config.switch_id,
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
        sleep(Duration::from_secs(1)).await;

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

trait BufferExt {
    fn clear(&mut self);
}

impl BufferExt for [u8] {
    fn clear(&mut self) {
        self.iter_mut().for_each(|byte| *byte = 0)
    }
}
