use std::{error::Error, sync::Arc, time::Duration};

use protocol::{Packet, Ping, VrfAction, CONFIGURATION_SWITCH_ID};
use tokio::{
    io::AsyncWriteExt,
    net::{TcpListener, TcpStream},
    select, spawn,
    sync::RwLock,
    time::sleep,
};

use crate::{
    cache::{SwitchTable, VrfTable},
    config::{Config, SwitchId},
    socket::{exchange_switch_id, TransmitPacket, PING_TIMEOUT},
    tap::{tap, TapTable},
    MAX_BUFFER_SIZE,
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

                let Some(client_switch_id) =
                    exchange_switch_id(&mut stream, config.switch_id).await
                else {
                    continue;
                };

                tracing::debug!("Client switch id {client_switch_id}");

                spawn(server_connection(
                    config.switch_id,
                    client_switch_id,
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
    server_switch_id: SwitchId,
    client_switch_id: SwitchId,
    mut stream: TcpStream,
    tap_table: Arc<RwLock<TapTable>>,
    vrf_table: Arc<RwLock<VrfTable>>,
    client_table: Arc<RwLock<ClientTable>>,
    switch_table: Arc<RwLock<SwitchTable>>,
) {
    let mut buffer = [0u8; MAX_BUFFER_SIZE];

    loop {
        let packet = select! {
            Some(packet) = stream.recv_packet(&mut buffer) => packet,
            _ = sleep(PING_TIMEOUT) => {
                tracing::warn!("Server connection closed, ping timed out");
                break
            },
            else => {
                tracing::warn!("Server connection closed");
                break
            },
        };

        tracing::debug!("{packet:?}");

        match packet {
            Packet::Ping(Ping) => {
                stream.send_packet(Ping).await;

                if let Err(error) = stream.flush().await {
                    tracing::warn!("Can't send ping: {error}");
                }
            }
            Packet::VrfAction(vrf_action) => {
                process_vrf_action(
                    server_switch_id,
                    client_switch_id,
                    &mut stream,
                    tap_table.clone(),
                    vrf_table.clone(),
                    client_table.clone(),
                    switch_table.clone(),
                    vrf_action,
                )
                .await
            }
            Packet::Data(data) => {
                let tap_table = tap_table.read().await;

                if let Some(tap) = tap_table.get(&data.vrf_id) {
                    if let Err(error) = tap.send((client_switch_id, data.data)).await {
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

async fn process_vrf_action(
    server_switch_id: SwitchId,
    client_switch_id: SwitchId,
    stream: &mut TcpStream,
    tap_table: Arc<RwLock<TapTable>>,
    vrf_table: Arc<RwLock<VrfTable>>,
    client_table: Arc<RwLock<ClientTable>>,
    switch_table: Arc<RwLock<SwitchTable>>,
    vrf_action: VrfAction,
) {
    if client_switch_id == CONFIGURATION_SWITCH_ID {
        match &vrf_action {
            VrfAction::Create(_)
            | VrfAction::Delete { .. }
            | VrfAction::AddMember { .. }
            | VrfAction::RemoveMember { .. } => {
                broadcast_packet(client_table.clone(), Packet::from(vrf_action.clone())).await
            }
            _ => {}
        }
    }

    match vrf_action {
        VrfAction::List(vrf_list) => {
            let vrf_table = vrf_table.read().await;

            for vrf_list_chunk in vrf_table.values().cloned().collect::<Vec<_>>().chunks(10) {
                stream
                    .send_packet(VrfAction::List(Some(vrf_list_chunk.to_vec())))
                    .await;
            }

            stream.send_packet(VrfAction::List(Some(Vec::new()))).await;

            if let Err(error) = stream.flush().await {
                tracing::warn!("Can't send vrf list: {error}");
            }
        }
        VrfAction::Create(vrf) => {
            let mut vrf_table = vrf_table.write().await;

            if !vrf_table.contains_key(&vrf.id)
                && vrf_table
                    .values()
                    .find(|vrf_| vrf_.name == vrf.name)
                    .is_none()
            {
                if vrf.members.contains(&server_switch_id) {
                    let mut tap_table = tap_table.write().await;

                    tap_table.insert(
                        vrf.id,
                        tap(vrf.clone(), client_table.clone(), switch_table.clone()),
                    );
                }

                vrf_table.insert(vrf.id, vrf);
            }
        }
        VrfAction::Delete { id } => {
            let mut vrf_table = vrf_table.write().await;
            let mut tap_table = tap_table.write().await;
            let mut switch_table = switch_table.write().await;

            tap_table.remove(&id);
            vrf_table.remove(&id);
            switch_table.remove(&id);
        }
        VrfAction::AddMember { id, members } => {
            let mut vrf_table = vrf_table.write().await;

            if let Some(vrf) = vrf_table.get_mut(&id) {
                for new_member in members {
                    if new_member == server_switch_id {
                        let mut tap_table = tap_table.write().await;

                        tap_table.insert(
                            vrf.id,
                            tap(vrf.clone(), client_table.clone(), switch_table.clone()),
                        );
                    }

                    if !vrf.members.contains(&new_member) {
                        vrf.members.push(new_member);
                    }
                }
            }
        }
        VrfAction::RemoveMember { id, members } => {
            let mut vrf_table = vrf_table.write().await;

            if let Some(vrf) = vrf_table.get_mut(&id) {
                for old_member in members {
                    if old_member == server_switch_id {
                        let mut tap_table = tap_table.write().await;

                        tap_table.remove(&vrf.id);
                    }

                    vrf.members.retain(|member| *member != old_member);
                }
            }
        }
    }
}

async fn broadcast_packet(client_table: Arc<RwLock<ClientTable>>, packet: Packet) {
    let client_table = client_table.read().await;

    for (switch_id, client) in client_table.iter() {
        if let Err(error) = client.send(packet.clone()).await {
            tracing::error!("Can't broadcast packet to switch id {}: {error}", switch_id);
        }
    }
}
