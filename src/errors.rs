use std::fmt;
use std::io;
use rustc_serialize::json;

/// The one and only error type for the lal library
///
/// Every command will raise one of these on failure, and these is some reuse between
/// commands for these errors. `Result<T, CliError>` is effectively the safety net
/// that every single advanced call goes through to avoid `panic!`
#[derive(Debug)]
pub enum CliError {
    /// Errors propagated from `std::fs` and `curl`
    Io(io::Error),
    /// Errors propagated from `rustc_serialize`
    Parse(json::DecoderError),

    // main errors
    /// Manifest file not found in working directory
    MissingManifest,
    /// Config not found in ~/.lal
    MissingConfig,
    /// Component not found in manifest
    MissingComponent(String),
    /// Manifest cannot be overwritten without forcing
    ManifestExists,

    // status/verify errors
    /// Core dependencies missing in INPUT
    MissingDependencies,
    /// Dependency present at wrong version
    InvalidVersion(String),
    /// Extraneous dependencies in INPUT
    ExtraneousDependencies(String),
    /// No lockfile found for a component in INPUT
    MissingLockfile(String),
    /// Multiple versions of a component was involved in this build
    MultipleVersions(String),
    /// Multiple environments was used to build a component
    MultipleEnvironments(String),
    /// Environment for a component did not match our expected environment
    EnvironmentMismatch(String, String),
    /// Custom versions are stashed in INPUT which will not fly on Jenkins
    NonGlobalDependencies(String),

    // env related errors
    /// Specified environment is not present in the main config
    MissingEnvironment(String),
    /// Default environment explicitly specified
    InvalidEnvironment,

    // build errors
    /// Build configurations does not match manifest or user input
    InvalidBuildConfiguration(String),

    // script errors
    /// Script not found in local .lal/scripts/ directory
    MissingScript(String),

    // cache errors
    /// Failed to find a tarball after fetching from artifactory
    MissingTarball,
    /// Failed to find build artifacts in OUTPUT after a build or before stashing
    MissingBuild,

    // stash errors
    /// Invalid integer name used with lal stash
    InvalidStashName(u32),
    /// Failed to find stashed artifact in the lal cache
    MissingStashArtifact(String),

    /// Shell errors from docker subprocess
    SubprocessFailure(i32),
    /// Docker permission gate
    DockerPermissionSafety(String),

    // fetch/update failures
    /// Unspecified install failure
    InstallFailure,
    /// Fetch failure related to artifactory
    ArtifactoryFailure(String),

    // publish errors
    /// Missing release build
    MissingReleaseBuild,
    /// Config missing artifactory credentials
    MissingArtifactoryCredentials,
}

// Format implementation used when printing an error
impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            CliError::Io(ref err) => err.fmt(f),
            CliError::Parse(ref err) => err.fmt(f),
            CliError::MissingManifest => write!(f, "No manifest.json found"),
            CliError::MissingConfig => write!(f, "No ~/.lal/config found"),
            CliError::MissingComponent(ref s) => {
                write!(f, "Component '{}' not found in manifest", s)
            }
            CliError::ManifestExists => write!(f, "Manifest already exists (use -f to force)"),
            CliError::MissingDependencies => write!(f, "Core dependencies missing in INPUT"),
            CliError::InvalidVersion(ref s) => {
                write!(f, "Dependency {} using incorrect version", s)
            }
            CliError::ExtraneousDependencies(ref s) => {
                write!(f, "Extraneous dependencies in INPUT ({})", s)
            }
            CliError::MissingLockfile(ref s) => write!(f, "No lockfile found in INPUT/{}", s),
            CliError::MultipleVersions(ref s) => {
                write!(f, "Depending on multiple versions of {}", s)
            }
            CliError::MultipleEnvironments(ref s) => {
                write!(f, "Depending on multiple environments to build {}", s)
            }
            CliError::EnvironmentMismatch(ref dep, ref env) => {
                write!(f, "Environment mismatch for {} - built in {}", dep, env)
            }
            CliError::NonGlobalDependencies(ref s) => {
                write!(f, "Depending on a custom version of {}", s)
            }
            CliError::MissingEnvironment(ref s) => {
                write!(f, "Environment '{}' not found in ~/.lal/config", s)
            }
            CliError::InvalidEnvironment => {
                write!(f, "Environment 'default' is reserved for internal use")
            }
            CliError::InvalidBuildConfiguration(ref s) => {
                write!(f, "Invalid build configuration - {}", s)
            }
            CliError::MissingScript(ref s) => {
                write!(f, "Missing script '{}' in local folder .lal/scripts/", s)
            }
            CliError::MissingTarball => write!(f, "Tarball missing in PWD"),
            CliError::MissingBuild => write!(f, "No build found in OUTPUT"),
            CliError::InvalidStashName(n) => {
                write!(f,
                       "Invalid name '{}' to stash under - must not be an integer",
                       n)
            }
            CliError::MissingStashArtifact(ref s) => {
                write!(f, "No stashed artifact '{}' found in ~/.lal/cache/stash", s)
            }
            CliError::SubprocessFailure(n) => write!(f, "Process exited with {}", n),
            CliError::DockerPermissionSafety(ref s) => {
                write!(f, "ID mismatch inside and outside docker - {}", s)
            }
            CliError::InstallFailure => write!(f, "Install failed"),
            CliError::ArtifactoryFailure(ref s) => write!(f, "Artifactory - {}", s),
            CliError::MissingReleaseBuild => write!(f, "Missing release build"),
            CliError::MissingArtifactoryCredentials => write!(f, "Missing artifactory credentials in ~/.lal/config"),
        }
    }
}

// Allow io and json errors to be converted to `CliError` in a try! without map_err
impl From<io::Error> for CliError {
    fn from(err: io::Error) -> CliError {
        CliError::Io(err)
    }
}

impl From<json::DecoderError> for CliError {
    fn from(err: json::DecoderError) -> CliError {
        CliError::Parse(err)
    }
}

/// Type alias to stop having to type out `CliError` everywhere.
///
/// Most functions can simply add the return type `LalResult<T>` for some `T`,
/// and enjoy the benefit of using `try!` or `?` without having to worry about
/// the many different error types that can arise from using curl, json serializers,
/// file IO, user errors, and potential logic bugs.
pub type LalResult<T> = Result<T, CliError>;
