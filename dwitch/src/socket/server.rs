use std::{error::Error, sync::Arc, time::Duration};

use protocol::{Packet, Ping, VrfAction};
use tokio::{
    io::AsyncReadExt,
    net::{TcpListener, TcpStream},
    spawn,
    sync::RwLock,
    time::sleep,
};

use crate::{
    cache::{self, SwitchTable, VrfTable},
    config::{Config, SwitchId},
    socket::exchange_switch_id,
    tap::{tap, TapTable},
    BufferExt, MAX_BUFFER_SIZE,
};

use super::client::ClientTable;

pub async fn server(
    config: Config,
    tap_table: Arc<RwLock<TapTable>>,
    vrf_table: Arc<RwLock<VrfTable>>,
    client_table: Arc<RwLock<ClientTable>>,
    switch_table: Arc<RwLock<SwitchTable>>,
) -> Result<(), Box<dyn Error>> {
    let listener = TcpListener::bind(config.listen).await?;

    loop {
        match listener.accept().await {
            Ok((mut stream, address)) => {
                tracing::debug!("New client from {address}");

                let Some(switch_id) = exchange_switch_id(&mut stream, config.switch_id).await
                else {
                    continue;
                };

                tracing::debug!("Client switch id {switch_id}");

                spawn(server_connection(
                    switch_id,
                    stream,
                    tap_table.clone(),
                    vrf_table.clone(),
                    client_table.clone(),
                    switch_table.clone(),
                ));
            }
            Err(error) => {
                tracing::error!("Can't accept client: {error}");
                sleep(Duration::from_secs(10)).await;
            }
        }
    }
}

async fn server_connection(
    switch_id: SwitchId,
    mut stream: TcpStream,
    tap_table: Arc<RwLock<TapTable>>,
    vrf_table: Arc<RwLock<VrfTable>>,
    client_table: Arc<RwLock<ClientTable>>,
    switch_table: Arc<RwLock<SwitchTable>>,
) {
    let mut buffer = [0u8; MAX_BUFFER_SIZE];

    loop {
        let length = match stream.read(&mut buffer).await {
            Ok(length) => length,
            Err(error) => {
                tracing::error!("Server read error: {error}");
                continue;
            }
        };

        if length == 0 {
            continue;
        }

        let buffer = buffer[..length].as_mut();
        let packet = match Packet::deserialize(&buffer) {
            Ok(packet) => packet,
            Err(error) => {
                tracing::error!("Can't deserialize packet: {error}");
                buffer.clear();
                continue;
            }
        };

        buffer.clear();

        tracing::debug!("{packet:?}");

        match packet {
            Packet::Ping(Ping) => unimplemented!(),
            Packet::Vrf(vrf) => {
                let mut vrf_table = vrf_table.write().await;

                match vrf.action {
                    VrfAction::Create(name, members) => {
                        let vrf = cache::Vrf {
                            id: vrf.id,
                            name: name,
                            members: members.into_iter().collect(),
                        };
                        let mut tap_table = tap_table.write().await;

                        if vrf.members.contains(&switch_id) {
                            tap_table.insert(
                                vrf.id,
                                tap(vrf.clone(), client_table.clone(), switch_table.clone()),
                            );
                        }

                        vrf_table.insert(vrf.id, vrf);
                    }
                    VrfAction::Delete => {
                        let mut tap_table = tap_table.write().await;

                        tap_table.remove(&vrf.id);
                        vrf_table.remove(&vrf.id);
                    }
                    VrfAction::AddMember(new_members) => {
                        if let Some(vrf) = vrf_table.get_mut(&vrf.id) {
                            let mut tap_table = tap_table.write().await;

                            if new_members.contains(&switch_id) {
                                tap_table.insert(
                                    vrf.id,
                                    tap(vrf.clone(), client_table.clone(), switch_table.clone()),
                                );
                            }

                            for new_member in new_members {
                                vrf.members.insert(new_member);
                            }
                        }
                    }
                    VrfAction::RemoveMember(old_members) => {
                        if let Some(vrf) = vrf_table.get_mut(&vrf.id) {
                            let mut tap_table = tap_table.write().await;

                            for old_member in old_members {
                                if old_member == switch_id {
                                    tap_table.remove(&vrf.id);
                                }

                                vrf.members.remove(&old_member);
                            }
                        }
                    }
                }
            }
            Packet::Data(data) => {
                let tap_table = tap_table.read().await;

                if let Some(tap) = tap_table.get(&data.vrf_id) {
                    if let Err(error) = tap.send((switch_id, data.data)).await {
                        tracing::error!(
                            "Can't send data to tap interface for vrf id {}: {error}",
                            data.vrf_id
                        );
                    }
                }
            }
        }
    }
}
