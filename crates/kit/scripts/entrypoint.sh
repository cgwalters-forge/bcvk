#!/bin/bash
set -euo pipefail

SELFEXE=/run/selfexe
TMPROOT=/run/tmproot

# Second stage, run by unshare below as PID 1 of a new PID and mount
# namespace. The namespace only exists to give us the hybrid root, not for
# isolation, so plain util-linux mounts and chroot are all we need.
if [[ "${1:-}" == "--in-namespace" ]]; then
    shift
    # Bind /run first, so its copy under the new root doesn't also
    # pick up the mounts made below.
    mount --rbind /run "$TMPROOT/run"
    mount -t proc proc "$TMPROOT/proc"
    mount --rbind /dev "$TMPROOT/dev"
    mount --rbind /var/tmp "$TMPROOT/var/tmp"
    mount -t tmpfs tmpfs "$TMPROOT/tmp"
    exec chroot "$TMPROOT" "$SELFEXE" container-entrypoint "$@"
fi

# Check for required binaries early
for bin in unshare mount chroot; do
    if ! command -v "$bin" &>/dev/null; then
        echo "Error: $bin (util-linux or coreutils) is required in the target container image" >&2
        exit 1
    fi
done

# Shell script library
init_tmproot() {
    if test -d /run/inner-shared; then return 0; fi
    # Should have been created by podman when initializing
    # the bind mount
    cd "$TMPROOT"

    # Create essential symlinks
    ln -sf usr/bin bin
    ln -sf usr/lib lib
    ln -sf usr/lib64 lib64
    ln -sf usr/sbin sbin
    mkdir -p {etc,var/tmp,dev,proc,run,sys,tmp}
    # Ensure we have /etc/passwd as ssh-keygen wants it for bad reasons
    systemd-sysusers --root $(pwd) &>/dev/null

    # Copy DNS configuration from container's /etc/resolv.conf (configured by podman --dns)
    # into the new root so QEMU's slirp can use it for DNS resolution
    if [ -f /etc/resolv.conf ]; then
        cp /etc/resolv.conf "$TMPROOT/etc/resolv.conf"
    fi

    # Shared directory between containers
    mkdir /run/inner-shared
}

# Pass ALL arguments to container-entrypoint
# Default to "run-ephemeral" if no args
if [[ $# -eq 0 ]]; then
    set -- "run-ephemeral"
    # Initialize environment
    init_tmproot
else
    # Other commands should wait for the other process
    # to create the temp root
    while test '!' -d /run/inner-shared; do sleep 0.1; done
fi

# Check systemd version from the container image (not host)
export SYSTEMD_VERSION=$(systemctl --version 2>/dev/null)

# Set up signal handlers that will cleanly exit on INT or TERM
trap 'kill -TERM $NS_PID 2>/dev/null; exit 0' INT TERM

# Run unshare in the background so we can handle signals. With --kill-child
# the kernel sends the supervisor SIGTERM if unshare goes away.
# Bash normally gives a background command /dev/null on stdin, so
# keep stdin attached so QEMU can receive input.
unshare --mount --propagation slave --pid --fork --kill-child=SIGTERM -- \
    "$0" --in-namespace "$@" <&0 &
NS_PID=$!

wait $NS_PID
