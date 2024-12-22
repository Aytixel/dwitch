use serde::{Deserialize, Serialize};

use common::{SwitchId, VrfId};

pub const CONFIGURATION_SWITCH_ID: SwitchId = 0;

macro_rules! packets {
    ($($packet_name:ident),*) => {
        #[derive(Debug, Clone, Deserialize, Serialize)]
        pub enum Packet {
            $($packet_name($packet_name)),*
        }

        $(
            impl From<$packet_name> for Packet {
                fn from(value: $packet_name) -> Self {
                    Self::$packet_name(value)
                }
            }
        )*
    };
}

packets!(Ping, Vrf, Data);

impl Packet {
    pub fn serialize(&self) -> Vec<u8> {
        bincode::serialize(self).expect("Can't serialize packet")
    }

    pub fn deserialize(bytes: &[u8]) -> bincode::Result<Self> {
        bincode::deserialize(bytes)
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Ping;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Vrf {
    pub id: VrfId,
    pub action: VrfAction,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub enum VrfAction {
    Create(String, Vec<SwitchId>),
    Delete,
    AddMember(Vec<SwitchId>),
    RemoveMember(Vec<SwitchId>),
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Data {
    pub vrf_id: VrfId,
    pub data: Vec<u8>,
}
