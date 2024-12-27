mod vrf;

use std::{
    io::{Read, Write},
    net::{SocketAddr, TcpStream},
};

use clap::{Parser, Subcommand};
use common::SwitchId;
use protocol::{PacketSerializer, CONFIGURATION_SWITCH_ID};
use vrf::VrfCommand;

#[derive(Parser)]
struct Args {
    /// Address of the dwitch daemon
    address: SocketAddr,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Vrf commands
    Vrf {
        #[command(subcommand)]
        command: VrfCommand,
    },
}

fn main() -> eyre::Result<()> {
    color_eyre::install()?;

    let args = Args::parse();
    let mut stream = TcpStream::connect(&args.address)?;
    let mut buffer = [0u8; SwitchId::BITS as usize];

    stream.write_all(&CONFIGURATION_SWITCH_ID.serialize())?;
    stream.read(&mut buffer)?;

    let _switch_id = SwitchId::deserialize(&buffer)?;

    match args.command {
        Command::Vrf { command } => vrf::command(command, stream),
    }?;

    Ok(())
}
