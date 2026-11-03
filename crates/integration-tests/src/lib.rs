//! Shared library code for bcvk integration tests.
//!
//! Re-exports the core test infrastructure from [`itest`] and adds
//! bcvk-specific constants.

// linkme (via itest) requires unsafe for distributed slices
#![allow(unsafe_code)]

// Re-export everything consumers need from itest so that test modules
// can continue to write `use integration_tests::integration_test;`.
pub use itest::image_to_test_suffix;
pub use itest::integration_test;
pub use itest::parameterized_integration_test;
pub use itest::IntegrationTest;
pub use itest::ParameterizedIntegrationTest;
pub use itest::TestFn;
pub use itest::INTEGRATION_TESTS;
pub use itest::PARAMETERIZED_INTEGRATION_TESTS;

/// Label used to identify containers created by integration tests.
pub const INTEGRATION_TEST_LABEL: &str = "bcvk.integration-test=1";

/// Label used to identify libvirt VMs created by integration tests.
pub const LIBVIRT_INTEGRATION_TEST_LABEL: &str = "bcvk-integration";

/// Capture command output without losing stderr on nonzero exit (as xshell's
/// output() would). Keep install errors and diagnostic paths in test reports.
pub fn output_or_bail(cmd: xshell::Cmd<'_>) -> anyhow::Result<std::process::Output> {
    let display = cmd.to_string();
    let output = cmd.ignore_status().output()?;
    anyhow::ensure!(
        output.status.success(),
        "command exited with {}: {display}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(output)
}

#[cfg(test)]
mod tests {
    #[test]
    fn captured_failure_retains_stderr() {
        let sh = xshell::Shell::new().unwrap();
        for code in [0, 7] {
            let script = format!("printf stdout-marker; printf stderr-marker >&2; exit {code}");
            let result = super::output_or_bail(xshell::cmd!(sh, "sh -c {script}"));
            if code == 0 {
                let output = result.unwrap();
                assert_eq!(output.stdout, b"stdout-marker");
                assert_eq!(output.stderr, b"stderr-marker");
            } else {
                let error = result.unwrap_err().to_string();
                assert!(error.contains("exit status: 7"), "{error}");
                assert!(error.contains("stdout:\nstdout-marker"), "{error}");
                assert!(error.contains("stderr:\nstderr-marker"), "{error}");
            }
        }
    }
}
