use protocol::PacketSerializer;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};

use crate::{config::SwitchId, MAX_BUFFER_SIZE};

pub mod client;
pub mod server;

async fn exchange_switch_id(stream: &mut TcpStream, switch_id: SwitchId) -> Option<SwitchId> {
    if let Err(error) = stream.write_all(&switch_id.serialize()).await {
        tracing::error!("Server can't send switch id: {error}");
        return None;
    }

    let mut buffer = [0u8; MAX_BUFFER_SIZE];
    let length = match stream.read(&mut buffer).await {
        Ok(length) => length,
        Err(error) => {
            tracing::error!("Client read error: {error}");
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
            tracing::error!("Can't deserialize switch_id: {error}");
            return None;
        }
    })
}
