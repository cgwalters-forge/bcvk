# Testing

## Prerequisites

Integration tests require KVM/QEMU and podman. For libvirt tests, ensure libvirtd is running and your user has libvirt group membership.

## Running Tests

```bash
# Unit tests
just test
cargo test --lib

# Integration tests (requires KVM, podman)
just test-integration
cargo test --test integration

# Specific test with output
cargo test test_name -- --nocapture
```

## Unit Tests

Place tests in `#[cfg(test)] mod tests` blocks within source files. Use nested modules to organize related tests.

## Integration Tests

Integration tests in `tests/` require network access to pull bootc images, working virtualization, and disk space. VMs need time to boot - account for this in test timeouts.

```bash
# Pull test images first
podman pull quay.io/fedora/fedora-bootc:42
```

## Manual Testing

```bash
# Ephemeral VM
bcvk ephemeral run -d --rm --name test quay.io/fedora/fedora-bootc:42
podman stop test

# SSH access
bcvk ephemeral run -d -K --name ssh-test quay.io/fedora/fedora-bootc:42
bcvk ephemeral ssh ssh-test "hostname"
podman stop ssh-test

# Disk image
bcvk to-disk quay.io/fedora/fedora-bootc:42 /tmp/test.img

# libvirt
bcvk libvirt run --name test quay.io/fedora/fedora-bootc:42
bcvk libvirt ssh test "uptime"
bcvk libvirt rm test
```

## Cleanup

```bash
# Remove test containers
podman ps -a | grep test | awk '{print $1}' | xargs -r podman rm -f

# Remove test disk images
rm -f /tmp/test-*.img

# Remove libvirt test VMs
virsh list --all | grep test | awk '{print $2}' | xargs -r virsh destroy
virsh list --all | grep test | awk '{print $2}' | xargs -r virsh undefine
```

## Debugging

```bash
# Run test with full output
cargo test test_name -- --nocapture --test-threads=1

# Debug logging
RUST_LOG=debug cargo test test_name

# Check KVM access
ls -la /dev/kvm

# Check VM is running
podman ps | grep test-vm
podman logs test-vm
```

### Failed `to-disk` installs

When `bootc install` fails inside the `to-disk` VM (this includes base disk
creation for `bcvk libvirt run`), bcvk saves the guest kernel log and the
virtiofsd logs to a `bcvk-to-disk-failure-*.log` file in the temporary
directory and prints its path. To also get the last 200 lines of each log on
stderr, pass `--debug`, enable debug logging, or set
`BCVK_PRINT_FAILURE_LOGS=1`. The integration tests set that variable, so the
logs show up in the output of a failing test.

## Coverage

```bash
cargo install cargo-tarpaulin
cargo tarpaulin --out Html
```