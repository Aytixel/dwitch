use std::{
    io::{Read, Write},
    net::TcpStream,
};

use clap::{Args, Subcommand};
use common::{SwitchId, VrfId};
use eyre::OptionExt;
use protocol::{Packet, PacketSerializer, Vrf, VrfAction};

#[derive(Subcommand)]
pub enum VrfCommand {
    /// List all vrfs
    List,

    /// Create a new vrf
    Create {
        /// Id of the vrf
        id: VrfId,

        /// Name of the vrf
        name: String,

        /// The list of switch ids where the vrf should be present
        members: Vec<SwitchId>,
    },

    /// Delete a vrf
    Delete {
        #[command(flatten)]
        id: VrfIdArg,
    },

    /// Action on members
    Member {
        #[command(flatten)]
        id: VrfIdArg,

        #[command(subcommand)]
        command: MemberCommand,
    },
}

#[derive(Subcommand)]
pub enum MemberCommand {
    /// Add members to the vrf
    Add {
        /// Switch ids to add
        members: Vec<SwitchId>,
    },
    /// Remove members from the vrf
    Remove {
        /// Switch ids to remove
        members: Vec<SwitchId>,
    },
}

pub fn command(command: VrfCommand, mut stream: TcpStream) -> eyre::Result<()> {
    match command {
        VrfCommand::List => {
            println!("Vrf list:");

            for Vrf { id, name, members } in list_vrf(&mut stream)? {
                println!("\t{id} - {name}: {members:?}");
            }
        }
        VrfCommand::Create { id, name, members } => {
            stream.write_all(
                &Packet::from(VrfAction::Create(Vrf { id, name, members })).serialize(),
            )?;
            stream.flush()?;
        }
        VrfCommand::Delete { id } => {
            let id = id.get(&mut stream)?;

            stream.write_all(&Packet::from(VrfAction::Delete { id }).serialize())?;
            stream.flush()?;
        }
        VrfCommand::Member { id, command } => {
            let id = id.get(&mut stream)?;

            stream.write_all(&match command {
                MemberCommand::Add { members } => {
                    Packet::from(VrfAction::AddMember { id, members }).serialize()
                }
                MemberCommand::Remove { members } => {
                    Packet::from(VrfAction::RemoveMember { id, members }).serialize()
                }
            })?;
            stream.flush()?;
        }
    }

    Ok(())
}

#[derive(Args)]
#[group(required = true, multiple = false)]
pub struct VrfIdArg {
    /// Id of the vrf
    #[arg(long)]
    id: Option<VrfId>,

    /// Name of the vrf
    #[arg(long)]
    name: Option<String>,
}

impl VrfIdArg {
    fn get(&self, stream: &mut TcpStream) -> eyre::Result<VrfId> {
        Ok(if let Some(name) = &self.name {
            list_vrf(stream)?
                .into_iter()
                .find(|vrf| vrf.name == *name)
                .ok_or_eyre("Can't find vrf with this name")?
                .id
        } else {
            self.id.unwrap()
        })
    }
}

fn list_vrf(stream: &mut TcpStream) -> eyre::Result<Vec<Vrf>> {
    stream.write_all(&Packet::from(VrfAction::List(None)).serialize())?;
    stream.flush()?;

    let mut vrf_list = Vec::new();

    loop {
        let mut buffer = [0u8; 65535];

        stream.read(&mut buffer)?;

        if let Packet::VrfAction(VrfAction::List(Some(vrf_list_chunk))) =
            Packet::deserialize(&buffer)?
        {
            if vrf_list_chunk.is_empty() {
                break;
            }

            vrf_list.extend(vrf_list_chunk);
        }
    }

    Ok(vrf_list)
}
