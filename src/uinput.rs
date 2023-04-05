use evdev_rs::{
    enums::{BusType, EventCode, EventType, EV_ABS, EV_KEY, EV_REL, EV_SYN},
    DeviceWrapper, InputEvent, TimeVal, UInputDevice, UninitDevice,
};

use crate::{JoystickConfig, MouseConfig};

pub struct UInputMouse {
    device: UInputDevice,
}

impl UInputMouse {
    pub fn new() -> Result<Self, std::io::Error> {
        let u = UninitDevice::new().expect("Unable to create an uninit device");

        u.set_name("Virtual Mouse");
        u.set_bustype(BusType::BUS_USB as u16);
        u.set_vendor_id(0xabcd);
        u.set_product_id(0xefef);

        // Note mouse keys have to be enabled for this to be detected
        // as a usable device, see: https://stackoverflow.com/a/64559658/6074942
        u.enable_event_type(&EventType::EV_KEY)?;
        u.enable_event_code(&EventCode::EV_KEY(EV_KEY::BTN_LEFT), None)?;
        u.enable_event_code(&EventCode::EV_KEY(EV_KEY::BTN_RIGHT), None)?;

        u.enable_event_type(&EventType::EV_REL)?;
        u.enable_event_code(&EventCode::EV_REL(EV_REL::REL_X), None)?;
        u.enable_event_code(&EventCode::EV_REL(EV_REL::REL_Y), None)?;

        u.enable_event_code(&EventCode::EV_SYN(EV_SYN::SYN_REPORT), None)?;

        let device = UInputDevice::create_from_device(&u)?;
        Ok(Self { device })
    }

    pub fn move_mouse_x(&mut self, x: i32) -> Result<(), std::io::Error> {
        self.move_mouse(EV_REL::REL_X, x)
    }

    pub fn move_mouse_y(&mut self, y: i32) -> Result<(), std::io::Error> {
        self.move_mouse(EV_REL::REL_Y, y)
    }

    // You doesnt NEED self to be mut, but i find it better for semantics
    fn move_mouse(&mut self, ev_rel: EV_REL, value: i32) -> Result<(), std::io::Error> {
        let time = TimeVal::try_from(std::time::SystemTime::now()).unwrap();
        self.device.write_event(&InputEvent {
            time,
            event_code: EventCode::EV_REL(ev_rel),
            value,
        })?;

        self.device.write_event(&InputEvent {
            time,
            event_code: EventCode::EV_SYN(EV_SYN::SYN_REPORT),
            value: 0,
        })?;

        Ok(())
    }
}

pub struct VMouseManager {
    vmouse: UInputMouse,
    ddx: f32,
    ddy: f32,
    dx: f32,
    dy: f32,
    speed_multiplier: f32,
    val_x: f32,
    val_y: f32,
}

impl VMouseManager {
    pub fn new(config: MouseConfig) -> Result<Self, std::io::Error> {
        Ok(Self {
            vmouse: UInputMouse::new()?,
            ddx: 0.0,
            ddy: 0.0,
            speed_multiplier: config.speed,
            dx: 0.0,
            dy: 0.0,
            val_x: 0.0,
            val_y: 0.0,
        })
    }

    pub fn map_event(&mut self, event: evdev_rs::InputEvent, joystick_config: &JoystickConfig) {
        let convert = |value: f32| -> f32 {
            let fcentered = value - joystick_config.offset;
            let sign = fcentered.signum();
            let fabs = fcentered.abs();

            let fabs = (fabs - joystick_config.dead_zone)
                / (joystick_config.max - joystick_config.dead_zone);

            sign * fabs.clamp(0.0, 1.0)
        };
        match event.event_code {
            EventCode::EV_ABS(EV_ABS::ABS_X) => self.val_x = event.value as f32,
            EventCode::EV_ABS(EV_ABS::ABS_Y) => self.val_y = event.value as f32,
            _ => (),
        }

        let (sin, cos) = f32::sin_cos(std::f32::consts::PI / 180. * joystick_config.angle);
        self.ddx = convert(self.val_x * cos + self.val_y * sin);
        self.ddy = convert(-self.val_x * sin + self.val_y * cos);
    }

    pub fn send_event(&mut self, dt: f32) -> Result<(), std::io::Error> {
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
