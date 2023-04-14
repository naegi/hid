use evdev::{
    uinput::{VirtualDevice, VirtualDeviceBuilder},
    AbsoluteAxisType, AttributeSet, EventType, InputEvent, InputEventKind, Key, RelativeAxisType,
};

use crate::{JoystickConfig, MouseConfig};

pub struct UInputMouse {
    device: VirtualDevice,
}

impl UInputMouse {
    pub fn new() -> Result<Self, std::io::Error> {
        let device = VirtualDeviceBuilder::new()?
            .name("Virtual mouse")
            .with_relative_axes(&AttributeSet::from_iter([
                RelativeAxisType::REL_X,
                RelativeAxisType::REL_Y,
            ]))?
            .with_keys(&AttributeSet::from_iter([Key::BTN_RIGHT, Key::BTN_LEFT]))?
            .build()?;

        Ok(Self { device })
    }

    pub fn move_mouse_x(&mut self, x: i32) -> Result<(), std::io::Error> {
        let input_event = InputEvent::new_now(EventType::RELATIVE, RelativeAxisType::REL_X.0, x);
        self.device.emit(&[input_event])
    }

    pub fn move_mouse_y(&mut self, y: i32) -> Result<(), std::io::Error> {
        let input_event = InputEvent::new_now(EventType::RELATIVE, RelativeAxisType::REL_Y.0, y);
        self.device.emit(&[input_event])
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
    pub fn new(config: &MouseConfig) -> Result<Self, std::io::Error> {
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

    pub fn map_event(&mut self, event: InputEvent, joystick_config: &JoystickConfig) {
        let convert = |value: f32| -> f32 {
            let fcentered = value - joystick_config.offset;
            let sign = fcentered.signum();
            let fabs = fcentered.abs();

            let fabs = (fabs - joystick_config.dead_zone)
                / (joystick_config.half_amplitude - joystick_config.dead_zone);

            sign * fabs.clamp(0.0, 1.0)
        };
        match event.kind() {
            InputEventKind::AbsAxis(AbsoluteAxisType::ABS_X) => self.val_x = event.value() as f32,
            InputEventKind::AbsAxis(AbsoluteAxisType::ABS_Y) => self.val_y = event.value() as f32,
            _ => (),
        }

        let (sin, cos) = f32::sin_cos(std::f32::consts::PI / 180. * joystick_config.angle);
        let v_x = convert(self.val_x);
        let v_y = convert(self.val_y);
        self.ddx = v_x * cos + v_y * sin;
        self.ddy = -v_x * sin + v_y * cos;
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
