use std::path::{Path, PathBuf};

use mio::{Interest, Poll, Token};

use crate::input_device::InputDevice;

pub struct InputDevicePool {
    token_start: usize,
    devices: Vec<InputDevice>,
}

impl InputDevicePool {
    pub fn new(token_start: usize) -> Self {
        Self {
            token_start,
            devices: Vec::new(),
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
    pub fn get(&self, token: Token) -> Option<&InputDevice> {
        self.devices.get(self.index_from_token(token))
    }

    pub fn insert_from_path(
        &mut self,
        poll: &mut Poll,
        path: PathBuf,
    ) -> Result<(), std::io::Error> {
        let device = InputDevice::new(path)?;
        let token = self.next_free_token();
        self.devices.push(device);
        poll.registry().register(
            self.devices.last_mut().unwrap(),
            token,
            Interest::WRITABLE | Interest::READABLE,
        )?;
        Ok(())
    }

    pub fn delete_from_path(&mut self, poll: &mut Poll, path: &Path) -> Result<(), std::io::Error> {
        for (i, device) in self.devices.iter().enumerate() {
            if device.path == path {
                return self.delete(poll, i);
            }
        }

        // NOT FOUND
        eprintln!("path not found in Input device Pool");
        Ok(())
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

        poll.registry()
            .deregister(&mut self.devices.pop().unwrap())?;
        Ok(())
    }
}
