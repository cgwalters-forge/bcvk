use std::process::{Command, Stdio};

use bootc_utils::CommandRunExt;
use color_eyre::{eyre::eyre, eyre::Context, Result};
use serde::{Deserialize, Serialize};

/// Policy for preparing a run's source image in local Podman storage.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, clap::ValueEnum, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PullPolicy {
    /// Pull only if the image is absent locally.
    #[default]
    Missing,
    /// Use local storage without pulling.
    Never,
    /// Pull every time.
    Always,
    /// Check the registry and pull if its image is newer.
    Newer,
}

impl PullPolicy {
    /// Decide whether to pull, and which Podman pull policy to use.
    fn decision(self, image: &str, present: bool) -> Result<Option<&'static str>> {
        if image.starts_with("localhost/") {
            return match self {
                Self::Always | Self::Newer => Err(eyre!(
                    "Cannot use --pull={} with local image {image}; use --pull=missing or --pull=never",
                    if self == Self::Always { "always" } else { "newer" }
                )),
                Self::Missing | Self::Never => Ok(None),
            };
        }
        Ok(match self {
            Self::Missing if !present => Some("--policy=missing"),
            Self::Missing | Self::Never => None,
            Self::Always => Some("--policy=always"),
            Self::Newer => Some("--policy=newer"),
        })
    }
}

fn image_exists(image: &str, mut command: Command) -> Result<bool> {
    let output = command
        .args(["image", "exists", "--", image])
        .output()
        .with_context(|| format!("Checking local image {image}"))?;
    // Podman distinguishes absence (1) from storage/command errors (125).
    match output.status.code() {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        _ => Err(eyre!(
            "podman image exists failed for {image} ({}): {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        )),
    }
}

/// Prepare an image before inspecting it or starting destructive run operations.
pub fn prepare_image(image: &str, policy: PullPolicy) -> Result<()> {
    prepare_image_with_command(image, policy, || Command::new("podman"))
}

fn prepare_image_with_command(
    image: &str,
    policy: PullPolicy,
    mut command: impl FnMut() -> Command,
) -> Result<()> {
    // Never and local-only references must retain the downstream local-image errors.
    let present = if policy == PullPolicy::Missing && !image.starts_with("localhost/") {
        image_exists(image, command())?
    } else {
        false
    };
    let Some(pull_arg) = policy.decision(image, present)? else {
        return Ok(());
    };
    if policy == PullPolicy::Missing {
        eprintln!("Image {image} is not present locally; pulling...");
    }
    // Use the same Podman executable and inherited environment as other operations,
    // including its normal auth files and REGISTRY_AUTH_FILE. Keep pull progress
    // visible on stderr without adding an image ID to the run's stdout.
    let status = command()
        .args(["pull", pull_arg, "--", image])
        .stdout(Stdio::null())
        .status()
        .with_context(|| format!("Pulling image {image}"))?;
    if !status.success() {
        return Err(eyre!("podman pull failed for image {image}: {status}"));
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Store {
    #[allow(dead_code)]
    pub graph_driver_name: String,
    pub graph_root: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PodmanSystemInfo {
    pub store: Store,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ImageInspect {
    pub size: u64,
}

pub fn get_system_info() -> Result<PodmanSystemInfo> {
    Command::new("podman")
        .arg("system")
        .arg("info")
        .arg("--format=json")
        .run_and_parse_json()
        .map_err(|e| eyre!("podman system info failed: {}", e))
}

/// Get the size of a container image in bytes
pub fn get_image_size(image: &str) -> Result<u64> {
    let inspect_result: Vec<ImageInspect> = Command::new("podman")
        .arg("inspect")
        .arg("--format=json")
        .arg("--type=image")
        .arg("--")
        .arg(image)
        .run_and_parse_json()
        .map_err(|e| eyre!("podman inspect failed for image {}: {}", image, e))?;

    if inspect_result.is_empty() {
        return Err(eyre!("No image found for: {}", image));
    }

    Ok(inspect_result[0].size)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prepare_image_commands() {
        use PullPolicy::*;

        const REMOTE: &str = "quay.io/example/os:latest";
        const LOCAL: &str = "localhost/example:latest";
        // Log each argument separately to catch missing `--` or image splitting.
        // All configuration is per-command; no process-wide environment changes.
        const STUB: &str = r#"
set -eu
printf '%s\n' "$@" >> "$CALLS"
printf 'auth=%s\n' "$REGISTRY_AUTH_FILE" >> "$CALLS"
case "$1" in
    image) printf 'fake storage failure\n' >&2; exit "$EXISTS_STATUS" ;;
    pull) exit "$PULL_STATUS" ;;
    *) exit 99 ;;
esac
"#;

        for (image, policy, exists_status, pull_status, calls, error) in [
            (REMOTE, Missing, 0, 0, &["exists"][..], None),
            (REMOTE, Missing, 1, 0, &["exists", "missing"][..], None),
            (
                REMOTE,
                Missing,
                125,
                0,
                &["exists"][..],
                Some(format!("podman image exists failed for {REMOTE} (exit status: 125): fake storage failure")),
            ),
            (REMOTE, Never, 125, 125, &[][..], None),
            (REMOTE, Always, 125, 0, &["always"][..], None),
            (REMOTE, Newer, 125, 0, &["newer"][..], None),
            (
                REMOTE,
                Missing,
                1,
                125,
                &["exists", "missing"][..],
                Some(format!("podman pull failed for image {REMOTE}: exit status: 125")),
            ),
            (
                REMOTE,
                Always,
                125,
                125,
                &["always"][..],
                Some(format!("podman pull failed for image {REMOTE}: exit status: 125")),
            ),
            (
                REMOTE,
                Newer,
                125,
                125,
                &["newer"][..],
                Some(format!("podman pull failed for image {REMOTE}: exit status: 125")),
            ),
            (LOCAL, Missing, 125, 125, &[][..], None),
            (LOCAL, Never, 125, 125, &[][..], None),
            (
                LOCAL,
                Always,
                125,
                125,
                &[][..],
                Some(format!("Cannot use --pull=always with local image {LOCAL}; use --pull=missing or --pull=never")),
            ),
            (
                LOCAL,
                Newer,
                125,
                125,
                &[][..],
                Some(format!("Cannot use --pull=newer with local image {LOCAL}; use --pull=missing or --pull=never")),
            ),
            ("localhost:5000/example:latest", Missing, 1, 0, &["exists", "missing"][..], None),
            ("-image with spaces", Missing, 1, 0, &["exists", "missing"][..], None),
        ] {
            let tempdir = tempfile::tempdir().unwrap();
            let log = tempdir.path().join("calls");
            let auth = tempdir.path().join("auth.json");
            std::fs::write(&log, "").unwrap();
            let mut command_count = 0;
            let result = prepare_image_with_command(image, policy, || {
                command_count += 1;
                let mut command = Command::new("sh");
                command
                    .args(["-c", STUB, "fake-podman"])
                    .env("CALLS", &log)
                    .env("EXISTS_STATUS", exists_status.to_string())
                    .env("PULL_STATUS", pull_status.to_string())
                    .env("REGISTRY_AUTH_FILE", &auth);
                command
            });
            let context = format!("{image}, {policy:?}, exists={exists_status}, pull={pull_status}");
            assert_eq!(result.map_err(|e| e.to_string()), error.map_or(Ok(()), Err), "{context}");
            assert_eq!(command_count, calls.len(), "{context}");

            let mut expected = String::new();
            for call in calls {
                let args = match *call {
                    "exists" => ["image", "exists", "--", image],
                    "missing" => ["pull", "--policy=missing", "--", image],
                    "always" => ["pull", "--policy=always", "--", image],
                    "newer" => ["pull", "--policy=newer", "--", image],
                    _ => unreachable!(),
                };
                expected.push_str(&format!("{}\nauth={}\n", args.join("\n"), auth.display()));
            }
            assert_eq!(std::fs::read_to_string(&log).unwrap(), expected, "{context}");
        }
    }

    #[test]
    fn test_pull_policy_decisions() {
        use PullPolicy::*;

        // Each row covers absent/present for registry and localhost references.
        for (policy, remote, local) in [
            (Missing, [Some("--policy=missing"), None], Ok(None)),
            (Never, [None, None], Ok(None)),
            (
                Always,
                [Some("--policy=always"); 2],
                Err("Cannot use --pull=always with local image localhost/example:latest; use --pull=missing or --pull=never"),
            ),
            (
                Newer,
                [Some("--policy=newer"); 2],
                Err("Cannot use --pull=newer with local image localhost/example:latest; use --pull=missing or --pull=never"),
            ),
        ] {
            for (index, present) in [false, true].into_iter().enumerate() {
                for image in ["quay.io/example/os:latest", "localhost:5000/example:latest", "example:latest"] {
                    assert_eq!(policy.decision(image, present).unwrap(), remote[index], "{policy:?}, {image}, present={present}");
                }
                assert_eq!(
                    policy.decision("localhost/example:latest", present).map_err(|e| e.to_string()),
                    local.map_err(str::to_owned),
                    "{policy:?}, present={present}"
                );
            }
        }
    }
}
