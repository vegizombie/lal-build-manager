use rustc_serialize::json;
use std::path::Path;
use std::fs::File;
use std::io::prelude::*;

use std::collections::HashMap;
use std::collections::BTreeSet;

use errors::{CliError, LalResult};
use util::input;

use rand;

/// Representation of a docker container image
#[derive(RustcDecodable, RustcEncodable, Debug, Clone)]
pub struct Container {
    /// The fully qualified image name
    pub name: String,
    /// The tag to use
    pub tag: String,
}

impl Container {
    /// Initialize a container struct
    ///
    /// This will split the container on `:` to actually fetch the tag, and if no tag
    /// was present, it will assume tag is latest as per docker conventions.
    pub fn new(container: &str) -> Container {
        let split: Vec<&str> = container.split(":").collect();
        let tag = if split.len() == 2 { split[1] } else { "latest" };
        let cname = if split.len() == 2 { split[0] } else { container };
        Container {
            name: cname.into(),
            tag: tag.into(),
        }
    }
}

/// Representation of `lockfile.json`
#[derive(RustcDecodable, RustcEncodable, Debug)]
pub struct Lockfile {
    /// Name of the component built
    pub name: String,
    /// Build configuration used
    pub config: String,
    /// Container and tag used to build
    pub container: Container,
    /// Version of the component built
    pub version: String,
    /// Version of the lal tool
    pub tool: String,
    /// Recursive map of dependencies used
    pub dependencies: HashMap<String, Lockfile>,
}

impl Lockfile {
    /// Initialize an empty Lockfile with defaults
    ///
    /// If no version is given, the version is EXPERIMENTAL+{randhex} for Colony.
    pub fn new(name: &str, container: &str, v: Option<&str>, build_cfg: Option<&str>) -> Lockfile {
        let def_version = format!("EXPERIMENTAL+{:x}", rand::random::<u64>());
        Lockfile {
            name: name.to_string(),
            version: v.unwrap_or(&def_version).to_string(),
            config: build_cfg.unwrap_or("release").to_string(),
            container: Container::new(container),
            tool: env!("CARGO_PKG_VERSION").to_string(),
            dependencies: HashMap::new(),
        }
    }
    /// Read all the lockfiles in INPUT to generate the full lockfile
    ///
    /// NB: This currently reads all the lockfiles partially in `analyze`,
    /// the re-reads them fully in `read_lockfile_from_component` so can be sped up.
    pub fn populate_from_input(mut self) -> LalResult<Self> {
        let deps = try!(input::analyze());
        for (name, _) in deps {
            trace!("Populating lockfile with {}", name);
            let deplock = try!(read_lockfile_from_component(&name));
            self.dependencies.insert(name.clone(), deplock);
        }
        Ok(self)
    }
    /// Write the current `Lockfile` struct to a Path
    pub fn write(&self, pth: &Path, silent: bool) -> LalResult<()> {
        let encoded = json::as_pretty_json(self);
        let mut f = try!(File::create(pth));
        try!(write!(f, "{}\n", encoded));
        if silent {
            debug!("Wrote lockfile {}: \n{}", pth.display(), encoded);
        } else {
            info!("Wrote lockfile {}: \n{}", pth.display(), encoded);
        }
        Ok(())
    }
}

// name of component -> (ver, other_ver, ..)
pub type DependencyUsage = HashMap<String, BTreeSet<String>>;

/// Recursive function used by `verify` to check for multiple version use
pub fn find_all_dependencies(lock: &Lockfile) -> DependencyUsage {
    let mut acc = HashMap::new();
    // for each entry in dependencies
    for (main_name, dep) in &lock.dependencies {
        // Store the dependency
        if !acc.contains_key(main_name) {
            acc.insert(main_name.clone(), BTreeSet::new());
        }
        {
            // Only borrow as mutable once - so creating a temporary scope
            let first_version_set = acc.get_mut(main_name).unwrap();
            first_version_set.insert(dep.version.clone());
        }

        // Recurse into its dependencies
        trace!("Recursing into deps for {}, acc is {:?}", main_name, acc);
        for (name, version_set) in find_all_dependencies(&dep) {
            trace!("Found versions for for {} under {} as {:?}",
                   name,
                   main_name,
                   version_set);
            // ensure each entry from above exists in current accumulator
            if !acc.contains_key(&name) {
                acc.insert(name.clone(), BTreeSet::new());
            }
            // union the entry of versions for the current name
            let full_version_set = acc.get_mut(&name).unwrap(); // know this exists now
            for version in version_set {
                full_version_set.insert(version);
            }
        }
    }
    acc
}


fn read_lockfile_from_component(component: &str) -> LalResult<Lockfile> {
    let lock_path = Path::new("./INPUT").join(component).join("lockfile.json");
    if !lock_path.exists() {
        return Err(CliError::MissingLockfile(component.to_string()));
    }
    let mut lock_str = String::new();
    try!(try!(File::open(&lock_path)).read_to_string(&mut lock_str));
    let res = try!(json::decode(&lock_str));
    Ok(res)
}
