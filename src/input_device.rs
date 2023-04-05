use std::{os::fd::AsRawFd, path::PathBuf};

use evdev_rs::{DeviceWrapper, ReadFlag};
use mio::{unix::SourceFd, Interest, Registry, Token};

pub struct InputDevice {
    pub path: PathBuf,
    pub device: evdev_rs::Device,
}

impl InputDevice {
    pub fn new(path: PathBuf) -> Result<Self, std::io::Error> {
        let device = evdev_rs::Device::new_from_path(&path)?;
        if let Some(n) = device.name() {
            println!(
                "Connected to device: '{}' ({:04x}:{:04x})",
                n,
                device.vendor_id(),
                device.product_id()
            );
        }
        Ok(Self { path, device })
    }

    fn as_raw_fd(&self) -> std::os::fd::RawFd {
        let evdev_ctx = self.device.raw();
        unsafe { evdev_sys::libevdev_get_fd(evdev_ctx) }.as_raw_fd()
    }

    pub fn next_event(&self) -> Result<Option<evdev_rs::InputEvent>, std::io::Error> {
        // TODO: take care of EAGAIN
        let next_event = self
            .device
            .next_event(ReadFlag::NORMAL | ReadFlag::BLOCKING);

        match next_event {
            Ok((_success, event)) => Ok(Some(event)),
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => Ok(None),
            Err(err) => Err(err),
        }
    }
}

impl mio::event::Source for InputDevice {
    fn register(
        &mut self,
        registry: &Registry,
        token: Token,
        interests: Interest,
    ) -> std::io::Result<()> {
        SourceFd(&self.as_raw_fd()).register(registry, token, interests)
    }

    fn reregister(
        &mut self,
        registry: &Registry,
        token: Token,
        interests: Interest,
    ) -> std::io::Result<()> {
        SourceFd(&self.as_raw_fd()).reregister(registry, token, interests)
    }

    fn deregister(&mut self, registry: &Registry) -> std::io::Result<()> {
        SourceFd(&self.as_raw_fd()).deregister(registry)
    }
}
