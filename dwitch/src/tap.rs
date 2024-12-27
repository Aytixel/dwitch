use std::{collections::HashMap, error::Error, io, sync::Arc};

use common::VrfId;
use netns::Netns;
use protocol::{Data, Packet, Vrf};
use tappers::{tokio::AsyncTap, DeviceState};
use tokio::{
    spawn,
    sync::{
        mpsc::{channel, Receiver, Sender},
        RwLock,
    },
};

use crate::{
    cache::{SwitchTable, VrfTable},
    config::SwitchId,
    socket::client::{broadcast_to_vrf, ClientTable},
    BufferExt, MAX_BUFFER_SIZE,
};

pub type TapTable = HashMap<VrfId, Sender<(SwitchId, Vec<u8>)>>;

pub fn initiate_tap_table(
    switch_id: SwitchId,
    vrf_table: &VrfTable,
    client_table: Arc<RwLock<ClientTable>>,
    switch_table: Arc<RwLock<SwitchTable>>,
) -> TapTable {
    let mut tap_table = HashMap::new();

    for (id, vrf) in vrf_table.iter() {
        if vrf.members.contains(&switch_id) {
            tap_table.insert(
                *id,
                tap(vrf.clone(), client_table.clone(), switch_table.clone()),
            );
        }
    }

    tap_table
}

pub fn tap(
    vrf: Vrf,
    client_table: Arc<RwLock<ClientTable>>,
    switch_table: Arc<RwLock<SwitchTable>>,
) -> Sender<(SwitchId, Vec<u8>)> {
    let (sender, receiver) = channel::<(SwitchId, Vec<u8>)>(32);

    match setup_tap(&vrf.name) {
        Ok(tap) => {
            spawn(tap_connection(
                tap,
                vrf,
                receiver,
                client_table.clone(),
                switch_table.clone(),
            ));
        }
        Err(error) => {
            tracing::error!("Error creating the tap for the vrf {}: {error}", vrf.name);
        }
    }

    sender
}

async fn tap_connection(
    tap: Tap,
    vrf: Vrf,
    mut receiver: Receiver<(SwitchId, Vec<u8>)>,
    client_table: Arc<RwLock<ClientTable>>,
    switch_table: Arc<RwLock<SwitchTable>>,
) {
    let tap = Arc::new(tap);

    let receiver_task = spawn({
        let tap = tap.clone();
        let vrf = vrf.clone();
        let switch_table = switch_table.clone();

        async move {
            let mut buffer = [0u8; MAX_BUFFER_SIZE];

            loop {
                if let Ok(length) = tap.recv(&mut buffer).await {
                    if length == 0 {
                        continue;
                    }

                    let buffer = &mut buffer[..length];

                    if length >= 14 {
                        let packet = Packet::from(Data {
                            vrf_id: vrf.id,
                            data: buffer.to_vec(),
                        });
                        let destination_mac = get_destination_mac(&buffer);

                        tracing::debug!("Destination mac address {destination_mac:?}");

                        if let Some(switch_id) = {
                            let switch_table = switch_table.read().await;

                            switch_table.get(&vrf.id).and_then(|vrf_switch_table| {
                                vrf_switch_table.get(&destination_mac).copied()
                            })
                        } {
                            let client_table = client_table.read().await;

                            if let Some(client) = client_table.get(&switch_id) {
                                if let Err(error) = client.send(packet).await {
                                    tracing::error!(
                                        "Can't send packet to client {switch_id} for vrf {}: {error}",
                                        vrf.name
                                    )
                                }
                            }
                        } else {
                            broadcast_to_vrf(&vrf, packet, client_table.clone()).await;
                        }
                    }

                    buffer.clear();
                }
            }
        }
    });

    while let Some((switch_id, data)) = receiver.recv().await {
        let source_mac = get_source_mac(&data);

        tracing::debug!("Source mac address {source_mac:?}");

        {
            let mut switch_table = switch_table.write().await;

            switch_table
                .entry(vrf.id)
                .or_default()
                .insert(source_mac, switch_id);
        }

        if let Err(error) = tap.send(&data).await {
            tracing::error!(
                "Can't send data through tap iterface for vrf {}: {error}",
                vrf.name
            );
        }
    }

    receiver_task.abort();
}

fn get_destination_mac(buffer: &[u8]) -> [u8; 6] {
    let mut mac = [0u8; 6];

    mac.copy_from_slice(&buffer[0..6]);
    mac
}

fn get_source_mac(buffer: &[u8]) -> [u8; 6] {
    let mut mac = [0u8; 6];

    mac.copy_from_slice(&buffer[6..12]);
    mac
}

fn setup_tap(netns_name: &str) -> Result<Tap, Box<dyn Error>> {
    let netns = Netns::named(netns_name);

    netns.create()?;

    let netns_handle = netns.enter()?;
    let mut tap = AsyncTap::new()?;

    tap.set_state(DeviceState::Up)?;

    netns_handle.close()?;

    Ok(Tap(tap, netns))
}

struct Tap(AsyncTap, Netns);

impl Tap {
    async fn send(&self, buf: &[u8]) -> io::Result<usize> {
        self.0.send(buf).await
    }

    async fn recv(&self, buf: &mut [u8]) -> io::Result<usize> {
        self.0.recv(buf).await
    }
}

impl Drop for Tap {
    fn drop(&mut self) {
        if let Err(error) = self.1.delete() {
            tracing::error!("Can't delete the netns {}: {error}", self.1);
        }
    }
}
