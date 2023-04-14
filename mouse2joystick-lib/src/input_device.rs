use std::{os::fd::AsRawFd, path::PathBuf};

use evdev::{Device, FetchEventsSynced};
use mio::{unix::SourceFd, Interest, Registry, Token};
use nix::fcntl::{FcntlArg, OFlag};

pub struct InputDevice {
    pub path: PathBuf,
    pub device: Device,
}

impl InputDevice {
    pub fn new(path: PathBuf) -> Result<Self, std::io::Error> {
        let device = Device::open(&path)?;

        Self::new_path_device(path, device)
    }
    pub fn new_path_device(path: PathBuf, device: Device) -> Result<Self, std::io::Error> {
        let raw_fd = device.as_raw_fd();
        //Make is non blocking
        nix::fcntl::fcntl(raw_fd, FcntlArg::F_SETFL(OFlag::O_NONBLOCK))?;
        Ok(Self { path, device })
    }

    pub fn events(&mut self) -> Result<FetchEventsSynced<'_>, std::io::Error> {
        self.device.fetch_events()
    }
}

impl mio::event::Source for InputDevice {
    fn register(
        &mut self,
        registry: &Registry,
        token: Token,
        interests: Interest,
    ) -> std::io::Result<()> {
        SourceFd(&self.device.as_raw_fd()).register(registry, token, interests)
    }

    fn reregister(
        &mut self,
        registry: &Registry,
        token: Token,
        interests: Interest,
    ) -> std::io::Result<()> {
        SourceFd(&self.device.as_raw_fd()).reregister(registry, token, interests)
    }

    fn deregister(&mut self, registry: &Registry) -> std::io::Result<()> {
        SourceFd(&self.device.as_raw_fd()).deregister(registry)
    }
}
