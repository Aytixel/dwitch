use std::{collections::HashMap, net::SocketAddr, sync::Arc};

use protocol::{Packet, Ping, Vrf};
use tokio::{
    net::TcpStream,
    select, spawn,
    sync::{
        mpsc::{channel, Sender},
        RwLock,
    },
    time::{sleep, sleep_until, Instant},
};

use crate::{
    config::SwitchId,
    socket::{
        exchange_switch_id, TransmitPacket, CONNECTION_RETRY_INTERVAL, PING_INTERVAL, PING_TIMEOUT,
    },
    MAX_BUFFER_SIZE,
};

pub type ClientTable = HashMap<SwitchId, Sender<Packet>>;

pub async fn client(
    switch_id: SwitchId,
    address: SocketAddr,
    client_table: Arc<RwLock<ClientTable>>,
) {
    let (sender, mut receiver) = channel::<Packet>(32);
    let mut buffer = [0u8; MAX_BUFFER_SIZE];

    loop {
        let mut stream = match TcpStream::connect(address).await {
            Ok(stream) => stream,
            Err(error) => {
                tracing::warn!("Can't connect to {address}: {error}");
                sleep(CONNECTION_RETRY_INTERVAL).await;
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

        spawn({
            let sender = sender.clone();

            async move {
                while let Ok(()) = sender.send(Packet::Ping(Ping)).await {
                    sleep(PING_INTERVAL).await;
                }
            }
        });

        let mut ping_timeout = Instant::now() + PING_TIMEOUT;

        loop {
            select! {
                Some(packet) = receiver.recv() => {
                    stream.send_packet(packet).await;
                }
                Some(Packet::Ping(Ping)) = stream.recv_packet(&mut buffer) => {
                    ping_timeout = Instant::now() + PING_TIMEOUT;
                }
                _ = sleep_until(ping_timeout) => {
                    tracing::warn!("Client connection closed, ping timed out");
                    break
                },
                else => {
                    tracing::warn!("Client connection closed");
                    break
                },
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
