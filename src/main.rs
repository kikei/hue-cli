extern crate config;
extern crate dirs;
extern crate philipshue;

use std::thread;
use std::time::Duration;
use structopt::StructOpt;
use philipshue::bridge::{self, Bridge};
use philipshue::errors::{HueError, HueErrorKind, BridgeError};
use philipshue::hue::LightCommand;

#[derive(StructOpt, Debug)]
#[structopt(name = "huecli", about = "CLI tool for control Philips Hue")]
struct Args {
    /// Show verbose
    #[structopt(short, long)]
    verbose: bool,

    #[structopt(subcommand)]
    cmd: Command
}

#[derive(StructOpt, Debug)]
enum Command {
    /// Discover bridge
    Discover,
    /// Register device and get user id
    Register {
        /// Host to register user
        #[structopt(short, long)]
        bridge: Option<String>,

        /// Device type
        #[structopt(short, long = "device-type")]
        device_type: String
    },
    /// Show lights
    Show {
        /// Host of bridge
        #[structopt(short, long)]
        bridge: Option<String>,

        /// Username registered to the devicE
        #[structopt(short, long)]
        user: Option<String>,

        /// Light id
        #[structopt(short, long)]
        id: Option<usize>,
    },
    /// Control light(s)
    Light {
        /// Host of bridge
        #[structopt(short, long)]
        bridge: Option<String>,

        /// Username registered to the devicE
        #[structopt(short, long)]
        user: Option<String>,

        /// Light id
        #[structopt(short, long)]
        id: usize,

        #[structopt(flatten)]
        state: LightState,
    }
}

#[derive(StructOpt, Debug)]
struct LightState {
    /// On/Off
    #[structopt(short, long, parse(try_from_str))]
    turn: Option<OnOff>,

    /// Brightness
    #[structopt(long)]
    bri: Option<u8>,

    /// Hue
    #[structopt(long)]
    hue: Option<u16>,

    /// Saturation
    #[structopt(long)]
    sat: Option<u8>,

    /// Color temperature [K]
    #[structopt(long)]
    ct: Option<u32>
}

#[derive(StructOpt, Debug)]
enum OnOff {
    On,
    Off
}

impl std::str::FromStr for OnOff {
    type Err = String;
    fn from_str(t: &str) -> Result<Self, Self::Err> {
        match t {
            "on" => Ok(OnOff::On),
            "off" => Ok(OnOff::Off),
            _ => Err("on or off is acceptable".to_string())
        }
    }
}

fn get_config_path() -> Option<String> {
    dirs::config_dir().as_mut().and_then(|p| {
        p.push("hue-cli");
        p.push("default.toml");
        p.to_str().map(|p| p.to_string())
    })
}

fn no_config() -> config::Config {
    config::Config::new()
}

fn load_config(path: &str) -> Result<config::Config, config::ConfigError> {
    let ok_default = Ok(no_config());
    let mut conf = config::Config::default();
    match conf.merge(config::File::with_name(path)) {
        Ok(conf) => Ok(conf.clone()),
        Err(config::ConfigError::NotFound(_)) => ok_default,
        Err(config::ConfigError::Foreign(_)) => ok_default,
        Err(e) => Err(e)
    }
}

fn main() {
    let args = Args::from_args();
    if args.verbose {
        println!("Arguments: {:?}", &args);
    };
    let conf_path = get_config_path();
    let conf = match conf_path {
        Some(ref path) => load_config(&path),
        None => Ok(no_config())
    };
    if args.verbose {
        println!("Configuration: {:?}", &conf);
    };
    match conf {
        Ok(conf) => dispatch(&args, &conf),
        Err(e) => println!("Configuration error: {:?}", e)
    }
}

fn or_config(v1: &Option<String>, v2: Option<String>) -> Option<String>
{
    v1.as_ref().map(|s| s.to_string()).or(v2)
}

fn dispatch(args: &Args, conf: &config::Config) {
    match &args.cmd {
        Command::Discover => discover(args),
        Command::Register { bridge, device_type } =>
            register(args, &bridge, &device_type),
        Command::Show { bridge, user, id } => {
            let bridge = or_config(&bridge, conf.get_str("bridge").ok());
            let user = or_config(&user, conf.get_str("user").ok());
            match (bridge, user) {
                (Some(h), Some(u)) => lights_show(args, &h, &u, &id),
                _ => println!("User and bridge must be specified")
            }
        },
        Command::Light { ref bridge, ref user, id, state } => {
            let bridge = or_config(&bridge, conf.get_str("bridge").ok());
            let user = or_config(&user, conf.get_str("user").ok());
            match (bridge, user) {
                (Some(h), Some(u)) =>
                    light_set(args, &h, &u, *id, &state),
                _ => println!("User and bridge must be specified")
            }
        }
    };
}

fn discover(_args: &Args) {
    let mut ips = bridge::discover_upnp().unwrap();
    ips.dedup();
    println!("Hue bridges found: {:#?}", ips);
}

fn register(args: &Args, bridge: &Option<String>, device_type: &String) {
    match try_register(args, bridge, device_type) {
        Ok((bridge, user)) => {
            println!(r#"Successfully user regestered: "{}"."#, &user);
            if let Some(toml) = get_config_path() {
                println!(
                    r#"I recommend you make default bridge setting \
                       such as following:\n\
                       ```\n\
                       cat <<EOF > {toml}\n\
                       user = "{user}"\n\
                       bridge = "{bridge}"\n\
                       EOF
                       ```"#, user=&user, bridge=&bridge, toml=&toml);
            }
        },
        Err(e) => println!("Failto register user, error: {}", &e)
    }
}

fn try_register(args: &Args, bridge: &Option<String>, device_type: &String)
    -> Result<(String, String), String> {
    let bridge = match bridge {
        Some(h) => Ok(h.to_string()),
        None => {
            if args.verbose {
                println!("Discovering bridge.");
            }
            bridge::discover_upnp()
                .map_err(|e| format!("{:}", &e))
                .and_then(|v| v.first().map(|f| f.to_string())
                          .ok_or("No bridge found".to_string()))
        }
    }?;
    if args.verbose {
        println!("Trying to register, bridge: {:}", &bridge);
    }
    let user = register_loop(&bridge, &device_type.as_str())?;
    Ok((bridge, user))
}

fn register_loop(bridge: &str, device_type: &str) -> Result<String, String> {
    let user: String;
    loop {
        match bridge::register_user(&bridge, device_type) {
            Ok(bridge) => {
                user = bridge.to_string();
                break;
            },
            Err(HueError(HueErrorKind::BridgeError {
                error: BridgeError::LinkButtonNotPressed, ..
            }, _)) => {
                println!("Please, press the link on the bridge. \
                          Retrying in 5 seconds");
                thread::sleep(Duration::from_secs(5));
            }
            Err(e) => return Err(format!("Unexpected error occured: {}", &e))
        };
    };
    Ok(user)
}

fn lights_show(args: &Args, bridge: &String, user: &String, id: &Option<usize>)
{
    let bridge = Bridge::new(bridge, user);
    match id {
        None => lights_get_all(args, &bridge),
        Some(id) => lights_get(args, &bridge, id)
    }
}

fn light_set(_args: &Args, bridge: &String, user: &String,
             id: usize, state: &LightState) {
    let bridge = Bridge::new(bridge, user);
    let mut cmd = LightCommand::default();
    match state.turn {
        Some(OnOff::On) => cmd = cmd.on(),
        Some(OnOff::Off) => cmd = cmd.off(),
        _ => ()
    }
    if let Some(bri) = state.bri {
        cmd = cmd.with_bri(bri);
    }
    if let Some(hue) = state.hue {
        cmd = cmd.with_hue(hue);
    }
    if let Some(sat) = state.sat {
        cmd = cmd.with_sat(sat);
    }
    if let Some(ct) = state.ct {
        cmd = cmd.with_ct((10000000u32 / ct) as u16);
    }
    match bridge.set_light_state(id, &cmd) {
        Ok(rsps) => for rsp in rsps.into_iter() {
            println!("{:?}", &rsp)
        },
        Err(e) => println!("Error {:?}", &e)
    }
}

fn lights_get_all(_args: &Args, bridge: &Bridge) {
    match bridge.get_all_lights() {
        Ok(lights) => {
            let max_name_len =
                lights.values()
                .map(|l| l.name.len())
                .chain(Some(4))
                .max()
                .unwrap();
            println!("id {0:1$} on  bri hue   sat ct    colormode xy",
                     "name",
                     max_name_len);
            for (id, light) in lights.iter() {
                println!("{id:2} {name:name_len$} {on:3} {bri:3} {hue:5} \
                          {sat:3} {ct:4}K {colormode:9} {xy:?}",
                         id=id,
                         name=light.name,
                         on=if light.state.on { "on" } else { "off" },
                         bri=light.state.bri,
                         hue=Show(&light.state.hue),
                         sat=Show(&light.state.sat),
                         ct=Show(&light.state.ct
                                   .map(|ct| 1000000u32 / ct as u32)),
                         colormode=Show(&light.state.colormode),
                         xy=Show(&light.state.xy),
                         name_len = max_name_len);
            }
        }
        Err(err) => println!("Error: {}", err),
    }
}

fn lights_get(_args: &Args, bridge: &Bridge, id: &usize) {
    match bridge.get_light(*id) {
        Ok(light) => {
            println!("id: {id:2}\n\
                      name: {name:}\n\
                      state:\n\
                      {indent:4}on: {on:3}\n\
                      {indent:4}bri: {bri:3}\n\
                      {indent:4}hue: {hue:5}\n\
                      {indent:4}sat: {sat:3}\n\
                      {indent:4}effect: {effect:}\n\
                      {indent:4}ct: {ct:4}K\n\
                      {indent:4}alert: {alert:}\n\
                      {indent:4}colormode: {colormode:9}\n\
                      {indent:4}xy: {xy:?}\n\
                      {indent:4}reachable: {reachable:}",
                     indent=" ",
                     id=id,
                     name=light.name,
                     on=light.state.on,
                     bri=light.state.bri,
                     hue=Show(&light.state.hue),
                     sat=Show(&light.state.sat),
                     effect=&light.state.effect.unwrap_or("N/A".to_string()),
                     ct=Show(&light.state.ct
                               .map(|ct| 1000000u32 / ct as u32)),
                     alert=&light.state.alert,
                     colormode=&light.state.colormode.unwrap_or("N/A".to_string()),
                     xy=Show(&light.state.xy),
                     reachable=&light.state.reachable);
        },
        Err(err) => println!("Error: {}", err),
    }
}

use std::fmt::{self, Display, Debug};

struct Show<'a, T: 'a>(&'a Option<T>);

impl<'a, T: 'a + Display> Display for Show<'a, T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self.0 {
            Some(ref x) => x.fmt(f),
            _ => Display::fmt("N/A", f),
        }
    }
}

impl<'a, T: 'a + Debug> Debug for Show<'a, T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self.0 {
            Some(ref x) => x.fmt(f),
            _ => Display::fmt("N/A", f),
        }
    }
}
