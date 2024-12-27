use std::{future::Future, time::Duration};

use protocol::{Packet, PacketSerializer};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};

use crate::{config::SwitchId, BufferExt, MAX_BUFFER_SIZE};

pub mod client;
pub mod server;

const CONNECTION_RETRY_INTERVAL: Duration = Duration::from_secs(1);
const PING_INTERVAL: Duration = Duration::from_secs(1);
const PING_TIMEOUT: Duration = Duration::from_secs(10);

async fn exchange_switch_id(stream: &mut TcpStream, switch_id: SwitchId) -> Option<SwitchId> {
    if let Err(error) = stream.write_all(&switch_id.serialize()).await {
        tracing::error!("Can't send switch id: {error}");
        return None;
    }

    let mut buffer = [0u8; MAX_BUFFER_SIZE];
    let length = match stream.read(&mut buffer).await {
        Ok(length) => length,
        Err(error) => {
            tracing::error!("Read error: {error}");
            return None;
        }
    };

    if length == 0 {
        return None;
    }

    let buffer = buffer[..length].as_mut();

    Some(match SwitchId::deserialize(&buffer) {
        Ok(switch_id) => switch_id,
        Err(error) => {
            tracing::error!("Can't deserialize switch id: {error}");
            return None;
        }
    })
}

pub trait TransmitPacket {
    fn recv_packet(&mut self, buffer: &mut [u8]) -> impl Future<Output = Option<Packet>>;

    fn send_packet<T: Into<Packet>>(&mut self, packet: T) -> impl Future<Output = ()>;
}

impl TransmitPacket for TcpStream {
    async fn recv_packet(&mut self, buffer: &mut [u8]) -> Option<Packet> {
        let length = match self.read(buffer).await {
            Ok(length) => length,
            Err(error) => {
                tracing::error!("Can't read from tcp stream: {error}");
                return None;
            }
        };

        if length == 0 {
            return None;
        }

        let buffer = buffer[..length].as_mut();
        let packet = match Packet::deserialize(&buffer) {
            Ok(packet) => packet,
            Err(error) => {
                tracing::error!("Can't deserialize packet: {error}");
                return None;
            }
        };

        buffer.clear();

        Some(packet)
    }

    async fn send_packet<T: Into<Packet>>(&mut self, packet: T) {
        if let Err(error) = self.write_all(&packet.into().serialize()).await {
            tracing::warn!("Can't send packet: {error}");
        }
    }
}
