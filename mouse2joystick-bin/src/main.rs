use mouse2joystick_lib::run_loop;

fn run() {
    // let f = std::fs::OpenOptions::new()
    //     .read(true)
    //     .write(false)
    //     .open("config.json");
    // let res = serde_json::from_reader(f)?;
    // let config = match read_config() {
    //     Ok(ok) => ok,
    //     Err(err) => {
    //         eprintln!("Error while reading config: {err}");
    //         Default::default()
    //     }
    // };

    run_loop();
}

fn main() {
    run();
}
