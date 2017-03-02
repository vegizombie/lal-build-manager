use std::fs;
use std::path::{Path, PathBuf};

use backend::{Component, Artifactory, Backend};
use super::{CliError, LalResult, Manifest};

pub fn download_to_path(url: &str, save: &PathBuf) -> LalResult<()> {
    use hyper::{self, Client};
    use std::io::prelude::{Write, Read};

    debug!("GET {}", url);
    let client = Client::new();
    let mut res = client.get(url).send()?;
    if res.status != hyper::Ok {
        return Err(CliError::ArtifactoryFailure(format!("GET request with {}", res.status)));
    }

    let mut buffer: Vec<u8> = Vec::new();
    res.read_to_end(&mut buffer)?;
    let mut f = fs::File::create(save)?;
    f.write_all(&buffer)?;
    Ok(())
}

// helper for fetch_and_unpack_component and stash::fetch_from_stash
pub fn extract_tarball_to_input(tarname: PathBuf, component: &str) -> LalResult<()> {
    use tar::Archive;
    use flate2::read::GzDecoder;

    let data = fs::File::open(tarname)?;
    let decompressed = GzDecoder::new(data)?; // decoder reads data
    let mut archive = Archive::new(decompressed); // Archive reads decoded

    let extract_path = Path::new("./INPUT").join(component);
    let _ = fs::remove_dir_all(&extract_path); // remove current dir if exists
    fs::create_dir_all(&extract_path)?;
    archive.unpack(&extract_path)?;
    Ok(())
}

// export a component from artifactory to stash
fn fetch_via_artifactory(backend: &Artifactory,
                         name: &str,
                         version: Option<u32>,
                         env: Option<&str>)
                         -> LalResult<(PathBuf, Component)> {
    use cache;

    trace!("Locate component {}", name);

    let component = backend.get_tarball_url(name, version, env)?;

    if !cache::is_cached(backend, &component.name, component.version, env) {
        // download to PWD then move it to stash immediately
        let local_tarball = Path::new(".").join(format!("{}.tar", name));
        download_to_path(&component.tarball, &local_tarball)?;
        cache::store_tarball(backend, name, component.version, env)?;
    }
    assert!(cache::is_cached(backend, &component.name, component.version, env),
            "cached component");

    trace!("Fetching {} from cache", name);
    let tarname = cache::get_cache_dir(backend, &component.name, component.version, env)
        .join(format!("{}.tar", name));
    Ok((tarname, component))
}

// import a component from stash to artifactory
fn fetch_and_unpack_component(backend: &Artifactory,
                              name: &str,
                              version: Option<u32>,
                              env: Option<&str>)
                              -> LalResult<Component> {
    let (tarname, component) = fetch_via_artifactory(backend, name, version, env)?;

    debug!("Unpacking tarball {} for {}",
           tarname.to_str().unwrap(),
           component.name);
    extract_tarball_to_input(tarname, name)?;

    Ok(component)
}

fn clean_input() {
    let input = Path::new("./INPUT");
    if input.is_dir() {
        fs::remove_dir_all(&input).unwrap();
    }
}

/// Update specific dependencies outside the manifest
///
/// Multiple "components=version" strings can be supplied, where the version is optional.
/// If no version is supplied, latest is fetched.
///
/// If installation was successful, the fetched tarballs are unpacked into `./INPUT`.
/// If one `save` or `savedev` was set, the fetched versions are also updated in the
/// manifest. This provides an easy way to not have to deal with strict JSON manually.
pub fn update(manifest: &Manifest,
              backend: &Artifactory,
              components: Vec<String>,
              save: bool,
              savedev: bool,
              env: &str)
              -> LalResult<()> {
    use cache;
    debug!("Update specific deps: {:?}", components);

    let mut error = None;
    let mut updated = Vec::with_capacity(components.len());
    for comp in &components {
        info!("Fetch {} {}", env, comp);
        if comp.contains('=') {
            let pair: Vec<&str> = comp.split('=').collect();
            if let Ok(n) = pair[1].parse::<u32>() {
                // standard fetch with an integer version
                match fetch_and_unpack_component(backend, pair[0], Some(n), Some(env)) {
                    Ok(c) => updated.push(c),
                    Err(e) => {
                        warn!("Failed to update {} ({})", pair[0], e);
                        error = Some(e);
                    }
                }
            } else {
                // fetch from stash - this does not go into `updated` it it succeeds
                // because we wont and cannot save stashed versions in the manifest
                let _ = cache::fetch_from_stash(backend, pair[0], pair[1]).map_err(|e| {
                    warn!("Failed to update {} from stash ({})", pair[0], e);
                    error = Some(e);
                });
            }
        } else {
            // fetch without a specific version (latest)
            match fetch_and_unpack_component(backend, comp, None, Some(env)) {
                Ok(c) => updated.push(c),
                Err(e) => {
                    warn!("Failed to update {} ({})", &comp, e);
                    error = Some(e);
                }
            }
        }
    }
    if error.is_some() {
        return Err(error.unwrap());
    }

    // Update manifest if saving in any way
    if save || savedev {
        let mut mf = manifest.clone();
        // find reference to correct list
        let mut hmap = if save { mf.dependencies.clone() } else { mf.devDependencies.clone() };
        for c in &updated {
            debug!("Successfully updated {} at version {}", &c.name, c.version);
            if hmap.contains_key(&c.name) {
                *hmap.get_mut(&c.name).unwrap() = c.version;
            } else {
                hmap.insert(c.name.clone(), c.version);
            }
        }
        if save {
            mf.dependencies = hmap;
        } else {
            mf.devDependencies = hmap;
        }
        mf.write()?;
    }
    Ok(())
}

/// Wrapper around update that updates all components
///
/// This will pass all dependencies or devDependencies to update.
/// If the save flag is set, then the manifest will be updated correctly.
/// I.e. dev updates will update only the dev portions of the manifest.
pub fn update_all(manifest: &Manifest,
                  backend: &Artifactory,
                  save: bool,
                  dev: bool,
                  env: &str)
                  -> LalResult<()> {
    let deps: Vec<String> = if dev {
        manifest.devDependencies.keys().cloned().collect()
    } else {
        manifest.dependencies.keys().cloned().collect()
    };
    update(manifest, backend, deps, save && !dev, save && dev, env)
}

/// Export a specific component from artifactory
pub fn export(backend: &Artifactory,
              comp: &str,
              output: Option<&str>,
              env: Option<&str>)
              -> LalResult<()> {
    use cache;
    let dir = output.unwrap_or(".");

    info!("Export {} {} to {}", env.unwrap_or("global"), comp, dir);

    let mut component_name = comp; // this is only correct if no =version suffix
    let tarname = if comp.contains('=') {
        let pair: Vec<&str> = comp.split('=').collect();
        if let Ok(n) = pair[1].parse::<u32>() {
            // standard fetch with an integer version
            component_name = pair[0]; // save so we have sensible tarball names
            fetch_via_artifactory(backend, pair[0], Some(n), env)?.0
        } else {
            // string version -> stash
            component_name = pair[0]; // save so we have sensible tarball names
            cache::get_path_to_stashed_component(backend, pair[0], pair[1])?
        }
    } else {
        // fetch without a specific version (latest)
        fetch_via_artifactory(backend, comp, None, env)?.0
    };

    let dest = Path::new(dir).join(format!("{}.tar.gz", component_name));
    debug!("Copying {:?} to {:?}", tarname, dest);

    fs::copy(tarname, dest)?;
    Ok(())
}

/// Remove specific components from `./INPUT` and the manifest.
///
/// This takes multiple components strings (without versions), and if the component
/// is found in `./INPUT` it is deleted.
///
/// If one of `save` or `savedev` was set, `manifest.json` is also updated to remove
/// the specified components from the corresponding dictionary.
pub fn remove(manifest: &Manifest, xs: Vec<&str>, save: bool, savedev: bool) -> LalResult<()> {
    debug!("Removing dependencies {:?}", xs);

    // remove entries in xs from manifest.
    if save || savedev {
        let mut mf = manifest.clone();
        let mut hmap = if save { mf.dependencies.clone() } else { mf.devDependencies.clone() };
        for component in xs.clone() {
            // We could perhaps allow people to just specify ANY dependency
            // and have a generic save flag, which we could infer from
            // thus we could modify both maps if listing many components

            // This could work, but it's not currently what install does, so not doing it.
            // => all components uninstalled from either dependencies, or all from devDependencies
            // if doing multiple components from different maps, do multiple calls
            if !hmap.contains_key(component) {
                return Err(CliError::MissingComponent(component.to_string()));
            }
            debug!("Removing {} from manifest", component);
            hmap.remove(component);
        }
        if save {
            mf.dependencies = hmap;
        } else {
            mf.devDependencies = hmap;
        }
        info!("Updating manifest with removed dependencies");
        mf.write()?;
    }

    // delete the folder (ignore if the folder does not exist)
    let input = Path::new("./INPUT");
    if !input.is_dir() {
        return Ok(());
    }
    for component in xs {
        let pth = Path::new(&input).join(component);
        if pth.is_dir() {
            debug!("Deleting INPUT/{}", component);
            fs::remove_dir_all(&pth)?;
        }
    }
    Ok(())
}

/// Fetch all dependencies from `manifest.json`
///
/// This will read, and HTTP GET all the dependencies at the specified versions.
/// If the `core` bool is set, then `devDependencies` are not installed.
pub fn fetch(manifest: &Manifest, backend: &Artifactory, core: bool, env: &str) -> LalResult<()> {
    use super::Lockfile;
    debug!("Installing dependencies{}",
           if !core { " and devDependencies" } else { "" });

    // create the joined hashmap of dependencies and possibly devdependencies
    let mut deps = manifest.dependencies.clone();
    if !core {
        for (k, v) in &manifest.devDependencies {
            deps.insert(k.clone(), *v);
        }
    }

    // figure out what we have already
    let lf = Lockfile::default().populate_from_input()
        .map_err(|e| {
            // Guide users a bit if they did something dumb - see #77
            warn!("Populating INPUT data failed - your INPUT may be corrupt");
            warn!("This can happen if you CTRL-C during `lal fetch`");
            warn!("Try to `rm -rf INPUT` and `lal fetch` again.");
            e
        })?;
    // filter out what we already have (being careful to examine env)
    for (name, d) in lf.dependencies {
        // if d.name at d.version in d.environment matches something in deps
        if let Some(&cand) = deps.get(&name) {
            // ignore extranous deps found in INPUT
            // ignore non-integer versions (stashed things must be overwritten)
            if let Ok(n) = d.version.parse::<u32>() {
                if n == cand && d.environment == env {
                    info!("Reuse {} {} {}", env, name, n);
                    deps.remove(&name);
                }
            }
        }
    }

    let mut err = None;
    for (k, v) in deps {
        info!("Fetch {} {} {}", env, k, v);

        // first kill the folders we actually need to fetch:
        let cmponent_dir = Path::new("./INPUT").join(&k);
        if cmponent_dir.is_dir() {
            // Don't think this can fail, but we are dealing with NFS
            fs::remove_dir_all(&cmponent_dir).map_err(|e| {
                    warn!("Failed to remove INPUT/{} - {}", k, e);
                    warn!("Please clean out your INPUT folder yourself to avoid corruption");
                    e
                })?;
        }

        let _ = fetch_and_unpack_component(backend, &k, Some(v), Some(env)).map_err(|e| {
            warn!("Failed to completely install {} ({})", k, e);
            // likely symlinks inside tarball that are being dodgy
            // this is why we clean_input
            err = Some(e);
        });
    }

    if err.is_some() {
        warn!("Cleaning potentially broken INPUT");
        clean_input(); // don't want to risk having users in corrupted states
        return Err(CliError::InstallFailure);
    }
    Ok(())
}
