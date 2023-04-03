mod uinput;

use std::{
    os::unix::prelude::FileTypeExt,
    path::{Path, PathBuf},
};

use evdev_rs::{
    enums::{EventCode, EV_ABS},
    DeviceWrapper, ReadFlag,
};
use mio::{unix::SourceFd, Events, Interest, Poll, Registry, Token};
use udev::{MonitorBuilder, MonitorSocket};
use uinput::UInputMouse;

fn process_udev_events(
    socket: &MonitorSocket,
    poll: &mut Poll,
    input_device_pool: &mut InputDevicePool,
) -> Result<(), std::io::Error> {
    for event in socket.iter() {
        match event.event_type() {
            udev::EventType::Add => {
                let device = event.device();
                let Some(devnode) = device.devnode() else {continue;};

                if device.sysname().to_str().unwrap().starts_with("event") {
                    println!("Device on devnode {:?} got added", devnode);
                    input_device_pool.insert_from_path(poll, devnode.to_owned())?;
                }
            }
            udev::EventType::Remove => {
                let device = event.device();
                let Some(devnode) = device.devnode() else {continue;};

                if device.sysname().to_str().unwrap().starts_with("event") {
                    println!("Device on devnode {:?} got removed", devnode);
                    input_device_pool.delete_from_path(poll, devnode)?;
                }
            }
            _ => (),
        }
    }
    Ok(())
}

struct InputDevice {
    path: PathBuf,
    device: evdev_rs::Device,
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
        use std::os::fd::AsRawFd;

        let evdev_ctx = self.device.raw();
        unsafe { evdev_sys::libevdev_get_fd(evdev_ctx) }.as_raw_fd()
    }

    fn next_event(&self) -> Result<Option<evdev_rs::InputEvent>, std::io::Error> {
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

struct InputDevicePool {
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

    fn contains(&self, token: Token) -> bool {
        self.index_from_token(token) < self.devices.len()
    }
    fn get(&self, token: Token) -> Option<&InputDevice> {
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

    fn delete_from_path(&mut self, poll: &mut Poll, path: &Path) -> Result<(), std::io::Error> {
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

    fn delete_from_token(&mut self, poll: &mut Poll, token: Token) -> Result<(), std::io::Error> {
        self.delete(poll, self.index_from_token(token))
    }
}

struct VMouseManager {
    vmouse: UInputMouse,
    ddx: f32,
    ddy: f32,
    dx: f32,
    dy: f32,
    speed_multiplier: f32,
}

impl VMouseManager {
    pub fn new() -> Result<Self, std::io::Error> {
        Ok(Self {
            vmouse: UInputMouse::new()?,
            ddx: 0.0,
            ddy: 0.0,
            speed_multiplier: 500.0,
            dx: 0.0,
            dy: 0.0,
        })
    }

    pub fn map_event(&mut self, event: evdev_rs::InputEvent) {
        fn convert(value: i32, dead_zone: i32, offset: i32, max: i32) -> f32 {
            let fval = value as f32;
            let foffset = offset as f32;
            let fdeadzone = dead_zone as f32;
            let fmax = max as f32;

            let fcentered = fval - foffset;
            let sign = fcentered.signum();
            let fabs = fcentered.abs();

            let fabs = (fabs - fdeadzone) / (fmax - fdeadzone);

            sign * fabs.clamp(0.0, 1.0)
        }
        match event.event_code {
            EventCode::EV_ABS(EV_ABS::ABS_X) => self.ddx = convert(event.value, 2000, 0, 35000),
            EventCode::EV_ABS(EV_ABS::ABS_Y) => self.ddy = convert(event.value, 2000, 0, 35000),
            _ => (),
        }
    }

    fn send_event(&mut self, dt: f32) -> Result<(), std::io::Error> {
        self.dx += dt * self.speed_multiplier * self.ddx;
        self.dy += dt * self.speed_multiplier * self.ddy;

        // println!("Move mouse with {dt} {} {}", self.ddx, self.ddy);
        if self.dx.abs() >= 1.0 {
            let dx = self.dx as i32;
            self.vmouse.move_mouse_x(dx)?;
            self.dx -= dx as f32;
        }

        if self.dy.abs() >= 1.0 {
            let dy = self.dy as i32;
            self.vmouse.move_mouse_y(dy)?;
            self.dy -= dy as f32;
        }
        Ok(())
    }
}

fn populate_from_dev_input(
    input_device_pool: &mut InputDevicePool,
    poll: &mut Poll,
) -> std::io::Result<()> {
    for entry in std::fs::read_dir("/dev/input/").unwrap() {
        let entry = entry?;
        if entry.file_type().unwrap().is_char_device()
            && entry.file_name().to_str().unwrap().starts_with("event")
        {
            input_device_pool.insert_from_path(poll, entry.path().to_owned())?;
        }
    }
    Ok(())
}

fn main() {
    let mut poll = Poll::new().expect("Can't create Poll");
    let mut events = Events::with_capacity(1024);

    let mut udev_socket = MonitorBuilder::new()
        .expect("Can't get a UDEV monitor")
        .match_subsystem("input")
        .expect("Could not match input subsystem")
        .listen()
        .expect("Could not listen");
    poll.registry()
        .register(
            &mut udev_socket,
            mio::Token(0),
            Interest::WRITABLE | Interest::READABLE,
        )
        .expect("Could not register udev socket for polling");

    let mut input_device_pool = InputDevicePool::new(1);
    populate_from_dev_input(&mut input_device_pool, &mut poll).expect("Can't populate");

    let mut vmouse_manager = VMouseManager::new().expect("Can't create vmouse");

    let mut last = std::time::Instant::now();
    loop {
        let now = std::time::Instant::now();
        let dt = (now - last).as_secs_f32();
        last = now;

        poll.poll(&mut events, Some(std::time::Duration::from_millis(10)))
            .expect("Could not poll");

        for event in &events {
            match event.token() {
                Token(0) => process_udev_events(&udev_socket, &mut poll, &mut input_device_pool)
                    .expect("Error while processing udev events"),
                token if input_device_pool.contains(token) => {
                    let Some(device) = input_device_pool.get(token) else {break;};
                    loop {
                        let event = device.next_event();

                        match event {
                            Err(err) => {
                                eprintln!(
                                    "unexpected error while getting input device next event: {err}"
                                );
                            }
                            Ok(Some(event)) => {
                                vmouse_manager.map_event(event);
                                continue;
                            }
                            Ok(None) => (),
                        }
                        break;
                    }
                }
                _ => eprintln!("Unknown poll token"),
            };
        }

        vmouse_manager
            .send_event(dt)
            .expect("Error while trying to send mouse movement");
    }
}
