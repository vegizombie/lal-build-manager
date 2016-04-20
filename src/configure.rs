use rustc_serialize::json;
use chrono::{Duration, UTC, DateTime};
use std::path::{Path, PathBuf};
use std::fs;
use std::env;
use std::io::prelude::*;
use errors::{CliError, LalResult};

/// Representation of `.lalrc`
#[allow(non_snake_case)]
#[derive(RustcDecodable, RustcEncodable, Clone)]
pub struct Config {
    /// Location of artifactory root
    pub artifactory: String, // TODO: use! hardcoded in artifactory.rs
    /// Cache directory for global and stashed builds
    pub cache: String,
    /// Docker container (potentially with tag) to use
    pub container: String,
    /// Time of last upgrade_check
    pub upgradeCheck: String,
}

impl Config {
    /// Initialize a Config with defaults
    ///
    /// This will locate you homedir, and set last update check 2 days in the past.
    /// Thus, with a blank default config, you will always trigger an upgrade check.
    pub fn new() -> LalResult<Config> {
        // unwrapping things that really must succeed here
        let home = env::home_dir().unwrap();
        let cachepath = Path::new(&home).join(".lal").join("cache");
        let cachedir = cachepath.as_path().to_str().unwrap();
        let time = UTC::now() - Duration::days(2);
        Ok(Config {
            artifactory: "http://engci-maven.cisco.com/artifactory/CME-group".to_string(),
            cache: cachedir.to_string(),
            container: "edonusdevelopers/centos_build:latest".to_string(),
            upgradeCheck: time.to_rfc3339(),
        })
    }
    /// Read and deserialize a Config from ~/.lal/lalrc
    pub fn read() -> LalResult<Config> {
        let home = env::home_dir().unwrap(); // crash if no $HOME
        let cfg_path = Path::new(&home).join(".lal/lalrc");
        if !cfg_path.exists() {
            return Err(CliError::MissingConfig);
        }
        let mut f = try!(fs::File::open(&cfg_path));
        let mut cfg_str = String::new();
        try!(f.read_to_string(&mut cfg_str));
        let res = try!(json::decode(&cfg_str));
        Ok(res)
    }
    /// Checks if it is time to perform an upgrade check
    pub fn upgrade_check_time(&self) -> bool {
        let last = self.upgradeCheck.parse::<DateTime<UTC>>().unwrap();
        let cutoff = UTC::now() - Duration::days(1);
        last < cutoff
    }
    /// Update the upgradeCheck time to avoid triggering it for another day
    pub fn performed_upgrade(&mut self) -> LalResult<()> {
        self.upgradeCheck = UTC::now().to_rfc3339();
        Ok(try!(self.write(true)))
    }
    /// Overwrite `~/.lal/lalrc` with serialized data from this struct
    pub fn write(&self, silent: bool) -> LalResult<()> {
        let home = env::home_dir().unwrap();
        let cfg_path = Path::new(&home).join(".lal").join("lalrc");

        let encoded = json::as_pretty_json(self);

        let mut f = try!(fs::File::create(&cfg_path));
        try!(write!(f, "{}\n", encoded));
        if silent {
            debug!("Wrote config {}: \n{}", cfg_path.display(), encoded);
        } else {
            info!("Wrote config {}: \n{}", cfg_path.display(), encoded);
        }
        Ok(())
    }
}


fn prompt(name: &str, default: String) -> String {
    use std::io::{self, Write};
    print!("Default {}: ({}) ", name, &default);
    io::stdout().flush().unwrap();

    let mut input = String::new();
    match io::stdin().read_line(&mut input) {
        Ok(n) => {
            if n > 1 {
                // more than just a newline character (which we strip)
                return (&input[0..n - 1]).to_string();
            }
        }
        Err(error) => println!("error: {}", error),
    }
    default
}

fn create_lal_dir() -> LalResult<PathBuf> {
    let home = env::home_dir().unwrap();
    let laldir = Path::new(&home).join(".lal");
    if !laldir.is_dir() {
        try!(fs::create_dir(&laldir));
    }
    Ok(laldir)
}

/// Create  `~/.lal/lalrc` interactively
///
/// This will prompt you interactively when setting `term_prompt`
/// Otherwise will just use the defaults.
///
/// A third boolean option to discard the output is supplied for tests.
pub fn configure(term_prompt: bool, save: bool) -> LalResult<Config> {
    let _ = try!(create_lal_dir());
    let mut cfg = try!(Config::new());

    if term_prompt {
        // Prompt for values:
        cfg.artifactory = prompt("artifactory", cfg.artifactory);
        cfg.cache = prompt("cache", cfg.cache);
        cfg.container = prompt("container", cfg.container);
    }
    if save {
        try!(cfg.write(false));
    }

    Ok(cfg.clone())
}
