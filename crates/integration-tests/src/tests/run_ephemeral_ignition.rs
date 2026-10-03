//! Integration tests for Ignition config injection

use integration_tests::integration_test;
use itest::TestResult;
use xshell::cmd;

use std::fs;
use tempfile::TempDir;

use camino::Utf8Path;

use crate::{ensure_image_present, get_bck_command, get_fcos_image, shell, INTEGRATION_TEST_LABEL};

/// Test that Ignition config injection mechanism works
///
/// This test verifies that the Ignition config injection mechanism is working
/// by checking that the ignition.platform.id=qemu kernel argument is set when
/// --ignition is specified. This works across all architectures.
///
/// Note: We don't test actual Ignition application here because FCOS won't
/// apply Ignition configs in ephemeral mode (treats it as subsequent boot).
/// The config injection works correctly for custom bootc images with Ignition.
fn test_run_ephemeral_ignition_works() -> TestResult {
    let sh = shell()?;
    let bck = get_bck_command()?;
    let label = INTEGRATION_TEST_LABEL;
    let fcos_image = get_fcos_image();

    // Normally prefetched (with retries) by `just pull-test-images`; only pull
    // if missing so a registry hiccup here can't fail the test.
    ensure_image_present(&sh, &fcos_image)?;

    // Create a temporary Ignition config
    let temp_dir = TempDir::new()?;
    let config_path = Utf8Path::from_path(temp_dir.path())
        .expect("temp dir is not utf8")
        .join("config.ign");

    // Minimal valid Ignition config (v3.3.0 for FCOS)
    let ignition_config = r#"{"ignition": {"version": "3.3.0"}}"#;
    fs::write(&config_path, ignition_config)?;

    // Check that the platform.id kernel arg is present
    let script = "/bin/sh -c 'grep -q ignition.platform.id=qemu /proc/cmdline && echo KARG_FOUND'";

    let stdout = cmd!(
        sh,
        "{bck} ephemeral run --rm --label {label} --ignition {config_path} --execute {script} {fcos_image}"
    )
    .read()?;

    assert!(
        stdout.contains("KARG_FOUND"),
        "Kernel command line should contain ignition.platform.id=qemu, got: {}",
        stdout
    );

    Ok(())
}
integration_test!(test_run_ephemeral_ignition_works);

/// Test that Ignition config validation rejects nonexistent files
fn test_run_ephemeral_ignition_invalid_path() -> TestResult {
    let sh = shell()?;
    let bck = get_bck_command()?;
    let label = INTEGRATION_TEST_LABEL;
    let fcos_image = get_fcos_image();

    // Normally prefetched (with retries) by `just pull-test-images`; only pull
    // if missing so a registry hiccup here can't fail the test.
    ensure_image_present(&sh, &fcos_image)?;

    let temp = TempDir::new()?;
    let nonexistent_path = Utf8Path::from_path(temp.path())
        .expect("temp dir is not utf8")
        .join("nonexistent-config.ign");

    let output = cmd!(
        sh,
        "{bck} ephemeral run --rm --label {label} --ignition {nonexistent_path} --karg systemd.unit=poweroff.target {fcos_image}"
    )
    .ignore_status()
    .output()?;

    assert!(
        !output.status.success(),
        "Should fail with nonexistent Ignition config file"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("not found"),
        "Error should mention missing file: {}",
        stderr
    );

    Ok(())
}
integration_test!(test_run_ephemeral_ignition_invalid_path);

/// Test that Ignition is rejected for images that don't support it
fn test_run_ephemeral_ignition_unsupported_image() -> TestResult {
    let sh = shell()?;
    let bck = get_bck_command()?;
    let label = INTEGRATION_TEST_LABEL;

    // Use standard bootc image that doesn't have Ignition support
    let image = "quay.io/centos-bootc/centos-bootc:stream10";

    let temp_dir = TempDir::new()?;
    let config_path = Utf8Path::from_path(temp_dir.path())
        .expect("temp dir is not utf8")
        .join("config.ign");

    let ignition_config = r#"{"ignition": {"version": "3.3.0"}}"#;
    fs::write(&config_path, ignition_config)?;

    let output = cmd!(
        sh,
        "{bck} ephemeral run --rm --label {label} --ignition {config_path} --karg systemd.unit=poweroff.target {image}"
    )
    .ignore_status()
    .output()?;

    assert!(
        !output.status.success(),
        "Should fail when using --ignition with non-Ignition image"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("does not support Ignition"),
        "Error should mention missing Ignition support: {}",
        stderr
    );

    Ok(())
}
integration_test!(test_run_ephemeral_ignition_unsupported_image);
