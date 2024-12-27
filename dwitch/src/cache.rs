use std::{collections::HashMap, error::Error, io};

use common::VrfId;
use protocol::Vrf;
use serde::{Deserialize, Serialize};
use tokio::fs::{read, write};

use crate::config::SwitchId;

const CACHE_PATH: &str = "/var/cache/dwitch.cache";

pub type SwitchTable = HashMap<VrfId, HashMap<[u8; 6], SwitchId>>;
pub type VrfTable = HashMap<VrfId, Vrf>;

#[derive(Debug, Default, Deserialize, Serialize)]
pub struct Cache {
    pub switch_table: SwitchTable,
    pub vrf_table: VrfTable,
}

impl Cache {
    pub async fn load() -> Result<Cache, Box<dyn Error>> {
        Ok(bincode::deserialize(&read(CACHE_PATH).await?)?)
    }

    pub async fn save(&self) -> io::Result<()> {
        Ok(write(
            CACHE_PATH,
            bincode::serialize(self).expect("Can't serialize cache"),
        )
        .await?)
    }
}
