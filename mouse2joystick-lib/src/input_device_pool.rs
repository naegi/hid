use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

use mio::{Interest, Poll, Token};

use crate::input_device::InputDevice;

pub struct InputDevicePool {
    pathes: HashSet<PathBuf>,
    token_start: usize,
    devices: Vec<InputDevice>,
}

impl InputDevicePool {
    pub fn new(token_start: usize) -> Self {
        Self {
            token_start,
            devices: Vec::new(),
            pathes: Default::default(),
        }
    }

    fn as_token(&self, index: usize) -> Token {
        Token(self.token_start + index)
    }

    fn index_from_token(&self, Token(tok): Token) -> usize {
        tok - self.token_start
    }

    fn next_free_token(&self) -> Token {
        self.as_token(self.devices.len())
    }

    pub fn contains(&self, token: Token) -> bool {
        self.index_from_token(token) < self.devices.len()
    }
    pub fn get_mut(&mut self, token: Token) -> Option<&mut InputDevice> {
        let index = self.index_from_token(token);
        self.devices.get_mut(index)
    }

    pub fn insert(&mut self, device: InputDevice, poll: &mut Poll) -> Result<(), std::io::Error> {
        if self.pathes.insert(device.path.clone()) {
            if let Some(n) = device.device.name() {
                println!(
                    "Connected to device: '{}' ({:04x}:{:04x})",
                    n,
                    device.device.input_id().vendor(),
                    device.device.input_id().product()
                );
            }
            let token = self.next_free_token();
            self.devices.push(device);
            poll.registry().register(
                self.devices.last_mut().unwrap(),
                token,
                Interest::WRITABLE | Interest::READABLE,
            )?;
        }
        Ok(())
    }

    pub fn find_path(&self, path: &Path) -> Option<usize> {
        for (i, device) in self.devices.iter().enumerate() {
            if device.path == path {
                return Some(i);
            }
        }

        None
    }

    pub fn delete_from_path(&mut self, poll: &mut Poll, path: &Path) -> Result<(), std::io::Error> {
        if let Some(index) = self.find_path(path) {
            self.delete(poll, index)?;
        }
        Ok(())
    }

    pub fn delete_from_token(
        &mut self,
        poll: &mut Poll,
        token: Token,
    ) -> Result<(), std::io::Error> {
        self.delete(poll, self.index_from_token(token))
    }

    // CRASH ON OOB
    fn delete(&mut self, poll: &mut Poll, index: usize) -> Result<(), std::io::Error> {
        let last_index = self.devices.len() - 1;
        if index != last_index {
            // Swap index and last
            self.devices.swap(index, last_index);

            let token = self.as_token(index);
            poll.registry().reregister(
                &mut self.devices[index],
                token,
                Interest::WRITABLE | Interest::READABLE,
            )?;
        }

        let mut poped_device = self.devices.pop().unwrap();

        if let Some(n) = poped_device.device.name() {
            println!(
                "Disconnecting device: '{}' ({:04x}:{:04x})",
                n,
                poped_device.device.input_id().vendor(),
                poped_device.device.input_id().product()
            );
        }
        self.pathes.remove(&poped_device.path);
        poll.registry().deregister(&mut poped_device)?;
        Ok(())
    }
}
