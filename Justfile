PRIMARY_IMAGE := "quay.io/centos-bootc/centos-bootc:stream10"
# TODO: Readd quay.io/almalinuxorg/almalinux-bootc:9.6 here after debugging
# <https://github.com/bootc-dev/bcvk/issues/153>
ALL_BASE_IMAGES := "quay.io/fedora/fedora-bootc:43 quay.io/fedora/fedora-bootc:44 quay.io/centos-bootc/centos-bootc:stream9 quay.io/centos-bootc/centos-bootc:stream10 quay.io/almalinuxorg/almalinux-bootc:10.0"
# Used by the Ignition tests, via BCVK_FCOS_IMAGE
FCOS_IMAGE := "quay.io/fedora/fedora-coreos:stable"

# Build the native binary
build:
   make

# Static checks
validate:
    make validate

# Run unit tests (excludes integration tests)
unit *ARGS:
    #!/usr/bin/env bash
    set -euo pipefail
    if command -v cargo-nextest &> /dev/null; then
        cargo nextest run {{ ARGS }}
    else
        cargo test {{ ARGS }}
    fi

# Pull all images used by the integration tests. Registries (notably quay.io)
# regularly return 5xx or drop connections mid-blob, and podman's own retries
# don't cover all of those, so retry each image with exponential backoff.
pull-test-images:
    #!/usr/bin/env bash
    set -euo pipefail
    attempts=5
    for image in {{ ALL_BASE_IMAGES }} {{ FCOS_IMAGE }}; do
        delay=15
        for ((i = 1; ; i++)); do
            podman pull -q "$image" >/dev/null && break
            if ((i >= attempts)); then
                echo "error: failed to pull $image after $attempts attempts" >&2
                exit 1
            fi
            echo "warning: pulling $image failed (attempt $i/$attempts), retrying in ${delay}s" >&2
            sleep "$delay"
            delay=$((delay * 2))
        done
    done

# Run integration tests (prefers cargo-nextest, falls back to cargo test with
# built-in fork-exec output capture)
test-integration *ARGS: build pull-test-images
    #!/usr/bin/env bash
    set -euo pipefail
    export BCVK_PATH=$(pwd)/target/release/bcvk
    export BCVK_PRIMARY_IMAGE={{ PRIMARY_IMAGE }}
    # Note: BCVK_ALL_IMAGES is quoted to preserve the space-separated list
    export BCVK_ALL_IMAGES="{{ ALL_BASE_IMAGES }}"
    export BCVK_FCOS_IMAGE={{ FCOS_IMAGE }}

    # Clean up any leftover containers before starting
    cargo run --release --bin test-cleanup -p integration-tests 2>/dev/null || true

    # Prefer nextest for better UX (retries, timing, etc.), but the harness
    # captures output itself via fork-exec so cargo test works too.
    if command -v cargo-nextest &> /dev/null; then
        cargo nextest run --release -P integration -p integration-tests {{ ARGS }} && TEST_EXIT_CODE=0 || TEST_EXIT_CODE=$?
    else
        cargo test --release -p integration-tests -- {{ ARGS }} && TEST_EXIT_CODE=0 || TEST_EXIT_CODE=$?
    fi

    # Clean up containers after tests complete (must run even on failure)
    cargo run --release --bin test-cleanup -p integration-tests 2>/dev/null || true

    exit $TEST_EXIT_CODE

# Clean up integration test containers
test-cleanup:
    cargo run --release --bin test-cleanup -p integration-tests

# Install cargo-nextest if not already installed
install-nextest:
    @which cargo-nextest > /dev/null 2>&1 || cargo install cargo-nextest --locked

# Run this before committing
fmt:
    cargo fmt

# Run the binary directly
run *ARGS:
    cargo run --release -- {{ ARGS }}

# Create archive with binary, tarball, and checksums
archive: build
    #!/usr/bin/env bash
    set -euo pipefail
    ARCH=$(arch)
    BINARY_PATH="target/release/bcvk"
    TARGET_NAME="bcvk-${ARCH}-unknown-linux-gnu"
    ARTIFACTS_DIR="target"
    
    # Strip the binary
    strip "${BINARY_PATH}" || true
    
    # Copy binary with target-specific name to artifacts directory
    cp "${BINARY_PATH}" "${ARTIFACTS_DIR}/${TARGET_NAME}"
    
    # Create tarball in artifacts directory
    cd "${ARTIFACTS_DIR}"
    tar -czf "${TARGET_NAME}.tar.gz" "${TARGET_NAME}"
    
    # Generate checksums
    sha256sum "${TARGET_NAME}.tar.gz" > "${TARGET_NAME}.tar.gz.sha256"
    
    # Clean up the temporary binary copy
    rm "${TARGET_NAME}"
    
    echo "Archive created: ${ARTIFACTS_DIR}/${TARGET_NAME}.tar.gz"
    echo "Checksum: ${ARTIFACTS_DIR}/${TARGET_NAME}.tar.gz.sha256"

# Install the binary to ~/.local/bin
install: build
    cp target/release/bcvk ~/.local/bin/

