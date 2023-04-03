use evdev_rs::{
    enums::{BusType, EventCode, EventType, EV_KEY, EV_REL, EV_SYN},
    DeviceWrapper, InputEvent, TimeVal, UInputDevice, UninitDevice,
};

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
