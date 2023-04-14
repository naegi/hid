pub mod input_device;
pub mod input_device_pool;
pub mod uinput;

use std::io::ErrorKind;
use std::os::fd::AsRawFd;

use crate::input_device::InputDevice;
use crate::input_device_pool::InputDevicePool;
use crate::uinput::VMouseManager;

use inotify::Inotify;
use inotify::WatchMask;
use mio::unix::SourceFd;
use mio::Events;
use mio::Interest;
use mio::Poll;
use mio::Token;

#[derive(serde::Deserialize)]
pub struct Config {
    pub mouse: MouseConfig,
    pub joystick: JoystickConfig,
}

#[derive(serde::Deserialize)]
pub struct MouseConfig {
    pub speed: f32,
}

#[derive(serde::Deserialize)]
pub struct JoystickConfig {
    pub dead_zone: f32,
    pub half_amplitude: f32,
    pub offset: f32,
    pub angle: f32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            mouse: MouseConfig { speed: 900.0 },
            joystick: JoystickConfig {
                dead_zone: 50.0,
                half_amplitude: 300.0,
                offset: 510.0,
                angle: -30.0,
            },
        }
    }
}

pub fn import_devices(
    input_device_pool: &mut InputDevicePool,
    poll: &mut Poll,
) -> std::io::Result<()> {
    for (path, device) in evdev::enumerate() {
        input_device_pool.insert(InputDevice::new_path_device(path, device)?, poll)?;
    }
    Ok(())
}

fn process_event(
    input_device_pool: &mut InputDevicePool,
    vmouse_manager: &mut VMouseManager,
    config: &Config,
    token: Token,
) -> Result<(), std::io::Error> {
    let Some(device) = input_device_pool.get_mut(token) else {return Ok(());};
    for event in device.events()? {
        vmouse_manager.map_event(event, &config.joystick);
    }
    Ok(())
}

#[no_mangle]
pub extern "C" fn run_loop() {
    let config = Config::default();
    let mut poll = Poll::new().expect("Can't create Poll");
    let mut events = Events::with_capacity(1024);

    let mut inotify = Inotify::init().expect("Failed to initialize an inotify instance");
    inotify
        .add_watch("/dev/input/", WatchMask::CREATE | WatchMask::ATTRIB)
        .expect("Can't watch /dev/input/?");
    poll.registry()
        .register(
            &mut SourceFd(&inotify.as_raw_fd()),
            Token(0),
            Interest::READABLE | Interest::WRITABLE,
        )
        .expect("Cant register inotify for polling");

    let mut input_device_pool = InputDevicePool::new(1);
    import_devices(&mut input_device_pool, &mut poll).expect("Can't populate");

    let mut vmouse_manager = VMouseManager::new(&config.mouse).expect("Can't create vmouse");

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
                Token(0) => import_devices(&mut input_device_pool, &mut poll)
                    .expect("Error while checking new devices"),
                token if input_device_pool.contains(token) => {
                    match process_event(&mut input_device_pool, &mut vmouse_manager, &config, token)
                    {
                        Ok(_) => (),
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => (),
                        Err(e) if e.raw_os_error() == Some(19) => {
                            println!("Cause: NOT FOUND");
                            input_device_pool
                                .delete_from_token(&mut poll, token)
                                .expect("Could not remove device from pool");
                        }
                        e => {
                            e.expect("Can't fetch events");
                        }
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

/// Expose the JNI interface for android below
#[cfg(target_os = "android")]
#[allow(non_snake_case)]
pub mod android {
    extern crate jni;

    use self::jni::objects::{JClass, JString};
    use self::jni::JNIEnv;
    use super::*;

    #[no_mangle]
    pub unsafe extern "C" fn Java_com_example_mouse2joystick_RustMouse2Joystick_run_1loop (
        _env: JNIEnv,
        _: JClass,
        _java_pattern: JString,
    ) {
        run_loop()
    }
}
