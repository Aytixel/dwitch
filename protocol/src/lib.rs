use serde::{de::DeserializeOwned, Deserialize, Serialize};

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

packets!(Ping, VrfAction, Data);

pub trait PacketSerializer: Sized + Serialize + DeserializeOwned {
    fn serialize(&self) -> Vec<u8> {
        bincode::serialize(self).expect("Can't serialize packet")
    }

    fn deserialize(bytes: &[u8]) -> bincode::Result<Self> {
        bincode::deserialize(bytes)
    }
}

impl PacketSerializer for Packet {}
impl PacketSerializer for SwitchId {}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Ping;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub enum VrfAction {
    List(Option<Vec<Vrf>>),
    Create(Vrf),
    Delete { id: VrfId },
    AddMember { id: VrfId, members: Vec<SwitchId> },
    RemoveMember { id: VrfId, members: Vec<SwitchId> },
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Vrf {
    pub id: VrfId,
    pub name: String,
    pub members: Vec<SwitchId>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Data {
    pub vrf_id: VrfId,
    pub data: Vec<u8>,
}
