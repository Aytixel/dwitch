use std::{
    error::Error,
    fmt::{self, Display, Formatter},
    fs::File,
    os::fd::BorrowedFd,
    path::{Path, PathBuf},
    process::exit,
};

use nix::{
    dir::Dir,
    fcntl::{open, OFlag},
    libc::IN_ISDIR,
    mount::{mount, umount2, MntFlags, MsFlags},
    sched::{setns, unshare, CloneFlags},
    sys::{
        stat::{stat, Mode},
        wait::waitpid,
    },
    unistd::{close, fork, mkdir, unlink, ForkResult},
};

const SELF_NETNS_PATH: &str = "/proc/self/ns/net";
const DEAULT_NETNS_PATH: &str = "/proc/1/ns/net";
const NETNS_PATH: &str = "/run/netns";

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Netns {
    #[default]
    Default,
    Named(String),
}

impl Netns {
    pub fn named<T: AsRef<str>>(netns_name: T) -> Self {
        Self::Named(netns_name.as_ref().to_string())
    }

    pub fn list() -> Vec<Netns> {
        let mut netns = vec![Netns::Default];
        let Ok(default_stat) = stat(DEAULT_NETNS_PATH) else {
            return netns;
        };
        let Ok(mut netns_dir) = Dir::open(
            NETNS_PATH,
            OFlag::O_RDONLY | OFlag::O_CLOEXEC | OFlag::O_DIRECTORY,
            Mode::empty(),
        ) else {
            return netns;
        };

        for entry in netns_dir.iter() {
            let Ok(entry) = entry else {
                continue;
            };
            let file_name = entry.file_name().to_string_lossy().to_string();

            if [".", ".."].contains(&file_name.as_str()) {
                continue;
            }

            let Ok(netns_stat) = stat(&Path::new(NETNS_PATH).join(&file_name)) else {
                continue;
            };

            if (netns_stat.st_mode & IN_ISDIR == IN_ISDIR)
                || (netns_stat.st_ino == default_stat.st_ino)
            {
                continue;
            }

            netns.push(Netns::Named(file_name));
        }

        netns
    }

    pub fn path(&self) -> PathBuf {
        match self {
            Netns::Default => Path::new(DEAULT_NETNS_PATH).to_path_buf(),
            Netns::Named(name) => Path::new(NETNS_PATH).join(name),
        }
    }

    pub fn exists(&self) -> bool {
        self.path().exists()
    }

    pub fn create(&self) -> nix::Result<()> {
        if self.exists() {
            return Ok(());
        }

        match unsafe { fork() }? {
            ForkResult::Parent { child, .. } => {
                waitpid(child, None)?;
            }
            ForkResult::Child => {
                if self.create_child().is_ok() {
                    exit(0);
                } else {
                    exit(1);
                }
            }
        }

        Ok(())
    }

    fn create_child(&self) -> nix::Result<()> {
        let netns_path = self.path();
        let netns_dir = Path::new(NETNS_PATH);

        // create netns directory if it doesn't exists
        if stat(netns_dir).is_err() {
            mkdir(
                netns_dir,
                Mode::S_IRWXU | Mode::S_IRGRP | Mode::S_IXGRP | Mode::S_IROTH | Mode::S_IXOTH,
            )?;
        }

        if mount(
            Some(Path::new("")),
            netns_dir,
            Some(Path::new("none")),
            MsFlags::MS_REC | MsFlags::MS_SHARED,
            None::<&Path>,
        )
        .is_err()
        {
            mount(
                Some(Path::new(netns_dir)),
                netns_dir,
                Some(Path::new("none")),
                MsFlags::MS_BIND | MsFlags::MS_REC,
                None::<&Path>,
            )?;
        }

        mount(
            Some(Path::new("")),
            netns_dir,
            Some(Path::new("none")),
            MsFlags::MS_REC | MsFlags::MS_SHARED,
            None::<&Path>,
        )?;

        // creating the netns
        let fd = open(
            &netns_path,
            OFlag::O_RDONLY | OFlag::O_CREAT | OFlag::O_EXCL,
            Mode::empty(),
        )?;

        if let Err(error) = close(fd) {
            unlink(&netns_path)?;

            return Err(error);
        }

        // unshare to the new netns
        if let Err(error) = unshare(CloneFlags::CLONE_NEWNET) {
            unlink(&netns_path)?;

            return Err(error);
        }

        let self_path = Path::new(SELF_NETNS_PATH);
        let fd = open(self_path, OFlag::O_RDONLY | OFlag::O_CLOEXEC, Mode::empty())?;

        // bind to the new netns
        if let Err(error) = mount(
            Some(self_path),
            &netns_path,
            Some(Path::new("none")),
            MsFlags::MS_BIND,
            None::<&Path>,
        ) {
            unlink(&netns_path)?;

            return Err(error);
        }

        if let Err(error) = setns(
            unsafe { BorrowedFd::borrow_raw(fd) },
            CloneFlags::CLONE_NEWNET,
        ) {
            unlink(&netns_path)?;

            return Err(error);
        }

        Ok(())
    }

    pub fn delete(&self) -> nix::Result<()> {
        if !self.exists() {
            return Ok(());
        }

        let netns_path = self.path();

        umount2(&netns_path, MntFlags::MNT_DETACH)?;
        unlink(&netns_path)
    }

    pub fn enter(&self) -> Result<NetnsHandle, Box<dyn Error>> {
        let initial_netns = File::open(SELF_NETNS_PATH)?;
        let target_netns = File::open(&self.path())?;

        unshare(CloneFlags::CLONE_NEWNET)?;
        setns(target_netns, CloneFlags::CLONE_NEWNET)?;

        Ok(NetnsHandle(initial_netns))
    }
}

impl Display for Netns {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Netns::Default => f.write_str("default"),
            Netns::Named(name) => f.write_str(&name),
        }
    }
}

pub struct NetnsHandle(File);

impl NetnsHandle {
    pub fn close(self) -> nix::Result<()> {
        unshare(CloneFlags::CLONE_NEWNET)?;
        setns(self.0, CloneFlags::CLONE_NEWNET)
    }
}
