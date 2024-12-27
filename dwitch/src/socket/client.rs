use std::{collections::HashMap, net::SocketAddr, sync::Arc, time::Duration};

use protocol::{Packet, PacketSerializer, Vrf};
use tokio::{
    io::AsyncWriteExt,
    net::TcpStream,
    sync::{
        mpsc::{channel, Sender},
        RwLock,
    },
    time::sleep,
};

use crate::{config::SwitchId, socket::exchange_switch_id};

pub type ClientTable = HashMap<SwitchId, Sender<Packet>>;

pub async fn client(
    switch_id: SwitchId,
    address: SocketAddr,
    client_table: Arc<RwLock<ClientTable>>,
) {
    let (sender, mut receiver) = channel::<Packet>(32);

    loop {
        let mut stream = match TcpStream::connect(address).await {
            Ok(stream) => stream,
            Err(error) => {
                tracing::warn!("Can't connect to {address}: {error}");
                sleep(Duration::from_secs(1)).await;
                continue;
            }
        };

        tracing::debug!("Client connected to {}", address);

        {
            let Some(switch_id) = exchange_switch_id(&mut stream, switch_id).await else {
                continue;
            };

            tracing::debug!("Server switch id {switch_id}");

            let mut client_table = client_table.write().await;

            client_table.insert(switch_id, sender.clone());
        }

        while let Some(packet) = receiver.recv().await {
            if let Err(error) = stream.write_all(&packet.serialize()).await {
                tracing::warn!("Failed to send packet to {address}: {error}");
            }
        }
    }
}

pub async fn broadcast_to_vrf(vrf: &Vrf, packet: Packet, client_table: Arc<RwLock<ClientTable>>) {
    let client_table = client_table.read().await;

    for member in vrf.members.iter() {
        if let Some(client) = client_table.get(member) {
            if let Err(error) = client.send(packet.clone()).await {
                tracing::error!(
                    "Can't send packet to client {} for vrf {}: {error}",
                    member,
                    vrf.name
                )
            }
        }
    }
}
