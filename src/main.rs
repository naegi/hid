pub mod input_device;
pub mod input_device_pool;
pub mod uinput;

use std::{
    io::{BufRead, ErrorKind, StdinLock},
    os::{fd::AsRawFd, unix::prelude::FileTypeExt},
};

use clap::Command;
use evdev_rs::enums::{EventCode, EV_ABS};
use input_device_pool::InputDevicePool;
use mio::{unix::SourceFd, Events, Interest, Poll, Token};
use udev::{MonitorBuilder, MonitorSocket};
use uinput::VMouseManager;

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

#[derive(serde::Deserialize)]
struct Config {
    mouse: MouseConfig,
    joystick: JoystickConfig,
}

#[derive(serde::Deserialize)]
pub struct MouseConfig {
    speed: f32,
}

#[derive(serde::Deserialize)]
pub struct JoystickConfig {
    pub dead_zone: f32,
    pub max: f32,
    pub offset: f32,
    pub angle: f32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            mouse: MouseConfig { speed: 700.0 },
            joystick: JoystickConfig {
                dead_zone: 0.0,
                max: i16::MAX as f32,
                offset: 0.0,
                angle: 0.0,
            },
        }
    }
}

fn read_config() -> Result<Config, Box<dyn std::error::Error>> {
    let f = std::fs::OpenOptions::new()
        .read(true)
        .write(false)
        .open("config.json")?;
    let res = serde_json::from_reader(f)?;
    Ok(res)
}

fn setup_udev(poll: &mut Poll, tok: usize) -> Result<MonitorSocket, std::io::Error> {
    let mut socket = MonitorBuilder::new()?.match_subsystem("input")?.listen()?;

    poll.registry().register(
        &mut socket,
        mio::Token(tok),
        Interest::WRITABLE | Interest::READABLE,
    )?;

    Ok(socket)
}

fn setup_stdin<'a, 'b>(poll: &'a mut Poll, tok: usize) -> Result<StdinLock<'b>, std::io::Error> {
    let stdin = std::io::stdin().lock();
    poll.registry().register(
        &mut SourceFd(&stdin.as_raw_fd()),
        mio::Token(tok),
        Interest::READABLE,
    )?;
    Ok(stdin)
}

fn run() {
    let config = match read_config() {
        Ok(ok) => ok,
        Err(err) => {
            eprintln!("Error while reading config: {err}");
            Default::default()
        }
    };

    let mut poll = Poll::new().expect("Can't create Poll");
    let mut events = Events::with_capacity(1024);

    let udev_socket = setup_udev(&mut poll, 0).expect("Could not setup udev for polling");

    let mut input_device_pool = InputDevicePool::new(1);
    populate_from_dev_input(&mut input_device_pool, &mut poll).expect("Can't populate");

    let mut vmouse_manager = VMouseManager::new(config.mouse).expect("Can't create vmouse");

    let mut last = std::time::Instant::now();
    loop {
        let now = std::time::Instant::now();
        let dt = (now - last).as_secs_f32();
        last = now;

        // NOTE: poll rate is 100HZ, maybe not the best ?
        match poll.poll(&mut events, Some(std::time::Duration::from_millis(10))) {
            Ok(_) => (),
            Err(err) if err.kind() == ErrorKind::Interrupted => (),
            err => err.expect("Error while polling"),
        }

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
                                vmouse_manager.map_event(event, &config.joystick);
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

fn calibrate() {
    let mut poll = Poll::new().expect("Can't create Poll");
    let mut events = Events::with_capacity(1024);

    let udev_socket = setup_udev(&mut poll, 0).expect("Could not setup udev");
    let mut stdin = setup_stdin(&mut poll, 1).expect("Could not register stdin for polling");

    let mut input_device_pool = InputDevicePool::new(2);
    populate_from_dev_input(&mut input_device_pool, &mut poll).expect("Can't populate");

    let mut max_x = f32::NEG_INFINITY;
    let mut max_y = f32::NEG_INFINITY;
    let mut min_x = f32::INFINITY;
    let mut min_y = f32::INFINITY;

    println!("Enter q to exit");
    'poll_loop: loop {
        poll.poll(&mut events, None).expect("Could not poll");

        for event in &events {
            match event.token() {
                Token(0) => process_udev_events(&udev_socket, &mut poll, &mut input_device_pool)
                    .expect("Error while processing udev events"),
                Token(1) => loop {
                    let mut buf = String::new();
                    if stdin.read_line(&mut buf).is_ok() {
                        if let Some('q') = buf.chars().next() {
                            break 'poll_loop;
                        }
                    } else {
                        break;
                    }
                },
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
                                match event.event_code {
                                    EventCode::EV_ABS(EV_ABS::ABS_X) => {
                                        let new_max_x = max_x.max(event.value as f32);
                                        let new_min_x = min_x.min(event.value as f32);

                                        if new_max_x > max_x {
                                            println!("NEW MAX_X VALUE: {new_max_x}");
                                            max_x = new_max_x;
                                        }
                                        if new_min_x < min_x {
                                            println!("NEW MAX_X VALUE: {new_max_x}");
                                            min_x = new_min_x;
                                        }
                                    }
                                    EventCode::EV_ABS(EV_ABS::ABS_Y) => {
                                        let new_max_y = max_y.max(event.value as f32);
                                        let new_min_y = min_y.min(event.value as f32);

                                        if new_max_y > max_y {
                                            println!("NEW MAX_Y VALUE: {new_max_y}");
                                            max_y = new_max_y;
                                        }
                                        if new_min_y < min_y {
                                            println!("NEW MAX_Y VALUE: {new_max_y}");
                                            min_y = new_min_y;
                                        }
                                    }
                                    _ => (),
                                }
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
    }

    println!("----------------");
    println!("X: max: {max_x}, min: {min_x}");
    println!("Y: max: {max_y}, min: {min_y}");
}

fn main() {
    let args = Command::new("hid")
        .subcommand_required(true)
        .arg_required_else_help(true)
        .subcommand(Command::new("run").about("Run the damn thing"))
        .subcommand(Command::new("calibrate").about("calibrate a joystick"));

    let matches = args.get_matches();

    match matches.subcommand() {
        Some(("run", _)) => run(),
        Some(("calibrate", _)) => calibrate(),
        _ => unreachable!(),
    }
}
