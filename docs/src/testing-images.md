# Testing Your Image

bcvk is a convenient way to run automated checks against a bootc image you
build, either locally or in CI. This page shows a few patterns that work well
from a `Justfile`, a `Makefile` or a CI job. The examples use
`quay.io/fedora/fedora-bootc:44`; substitute your own image, for example
`localhost/myos` after a `podman build -t localhost/myos .`.

For the simplest possible check, that the image boots and systemd reaches the
"running" state with no failed units, use
[bcvk ephemeral test-basic](./man/bcvk-ephemeral-test-basic.md).

## Running a test script in an ephemeral VM

`bcvk ephemeral run-ssh IMAGE -- COMMAND` boots the image, runs the command
over SSH as root, removes the VM, and exits with the command's exit status.
That makes it a good fit for a smoke test that does not need the image to be
installed to disk.

Anything beyond a one-liner is easier to keep in a script in your repository,
for example `tests/smoke.sh`:

```bash
#!/bin/bash
# Runs inside the VM, as root.
set -euo pipefail
# Don't hang waiting for a pager if SSH allocated a terminal.
export SYSTEMD_PAGER=

: "${EXPECT_SERVICE:?}"
systemctl is-active "${EXPECT_SERVICE}"
command -v bootc podman
echo "smoke test passed"
```

The command after `--` is handed to SSH as a single string and parsed again
by the shell in the VM, so quoting a multi-line script through `just`, the
host shell and SSH quickly gets fragile. Encoding the script with base64
sidesteps that, since the encoded text has no characters any of those layers
interpret:

```bash
script=$(base64 -w0 tests/smoke.sh)
bcvk ephemeral run-ssh quay.io/fedora/fedora-bootc:44 -- \
    "echo ${script} | base64 -d | env EXPECT_SERVICE=sshd.service bash"
```

Pass parameters with `env` as shown rather than by splicing them into the
script text.

An ephemeral VM boots the container image directly instead of installing it,
so checks that depend on the installed system, such as the booted image
reported by `bootc status` or the root filesystem type, belong in a persistent
VM instead.

## Checking an installed VM

`bcvk libvirt run` installs the image to a disk with `bootc install` and boots
it under libvirt. A few options make it convenient to script:

- `--replace` removes an existing VM of the same name first, so rerunning the
  recipe after rebuilding the image starts from a fresh install.
- `--detach --ssh-wait` returns once SSH works, so the next command can
  connect immediately.
- `--target-transport containers-storage` makes the installed system track
  the image in containers-storage, which is what you want for an image you
  built locally and have not pushed.
- `--filesystem btrfs` (or `xfs`, `ext4`) selects the root filesystem. It is
  required for images that do not configure a default, such as
  `fedora-bootc`.

`bcvk libvirt ssh NAME COMMAND` then runs checks in the VM, and exits with
their status:

```bash
bcvk libvirt run --replace --name myos-test \
    --target-transport containers-storage --filesystem btrfs \
    --detach --ssh-wait quay.io/fedora/fedora-bootc:44
bcvk libvirt ssh myos-test 'set -eu
    bootc status --format=json |
        jq -e ".status.booted.image.image.image == \"quay.io/fedora/fedora-bootc:44\""
    test "$(findmnt -no FSTYPE /sysroot)" = btrfs
    systemctl is-active sshd'
bcvk libvirt rm --force myos-test
```

The base64 approach from the previous section works with `bcvk libvirt ssh`
as well.

## A Justfile

Putting these together, a `Justfile` for an image repository might look like:

```just
image := env("IMAGE", "localhost/myos")
vm := "myos-test"

build:
    podman build -t {{ image }} .

# Boot the image without installing it and run tests/smoke.sh.
ephemeral-smoke:
    #!/bin/bash
    set -euo pipefail
    script=$(base64 -w0 tests/smoke.sh)
    bcvk ephemeral run-ssh {{ image }} -- "echo ${script} | base64 -d | env EXPECT_SERVICE=sshd.service bash"

# Install the image into a persistent VM; reruns replace it.
vm-create:
    bcvk libvirt run --replace --name {{ vm }} --target-transport containers-storage --filesystem btrfs --detach --ssh-wait {{ image }}

vm-test:
    bcvk libvirt ssh {{ vm }} 'set -eu; bootc status --format=json | jq -e ".status.booted.image.image.image == \"{{ image }}\""; systemctl is-active sshd'

vm-clean:
    bcvk libvirt rm --force {{ vm }}
```

Keeping the persistent VM around between `vm-create` and `vm-test` is handy
while iterating, since you can also `bcvk libvirt ssh myos-test` into it to
look around.

## Disposable VMs in CI

In CI, or for a long test you might run several of at once, it is better to
give each run its own VM and remove it when the run ends. Two things make the
cleanup safe. A unique name keeps concurrent runs from colliding. And recording
the VM's libvirt UUID right after creating it lets the cleanup trap remove
only that VM: if the name now refers to a different domain (because another
job reused it, or something ran `bcvk libvirt run --replace` with the same
name, which creates a new domain), the trap leaves it alone and fails the run
instead.

```bash
#!/bin/bash
set -euo pipefail
image=${1:?usage: $0 IMAGE}
connect=${LIBVIRT_DEFAULT_URI:-qemu:///session}
name="myos-test-$(< /proc/sys/kernel/random/uuid)"
domain_uuid=''

cleanup() {
    local status=$?
    if [[ -z "${domain_uuid}" ]]; then
        exit "${status}"
    fi
    if ((status != 0)) && [[ "${KEEP_FAILED_VM:-0}" == 1 ]]; then
        echo "Keeping failed VM ${name}" >&2
    elif [[ "$(virsh -c "${connect}" domuuid "${name}" 2>/dev/null)" != "${domain_uuid}" ]]; then
        echo "Not removing ${name}: its libvirt UUID changed" >&2
        status=1
    elif ! bcvk libvirt --connect "${connect}" rm --force "${name}"; then
        echo "Failed to remove ${name}" >&2
        status=1
    fi
    exit "${status}"
}
trap cleanup EXIT

bcvk libvirt --connect "${connect}" run --name "${name}" \
    --target-transport containers-storage --filesystem btrfs \
    --detach --ssh-wait "${image}"
domain_uuid=$(virsh -c "${connect}" domuuid "${name}")

bcvk libvirt --connect "${connect}" ssh "${name}" 'systemctl is-active sshd'
```

Without `--replace`, `bcvk libvirt run` fails if a VM with that name already
exists, so the script never takes over a VM it did not create. Set
`KEEP_FAILED_VM=1` to keep the VM of a failed run for debugging, then remove
it with `bcvk libvirt rm --force NAME`.

## See Also

- [bcvk-ephemeral-run-ssh(8)](./man/bcvk-ephemeral-run-ssh.md)
- [bcvk-ephemeral-test-basic(8)](./man/bcvk-ephemeral-test-basic.md)
- [bcvk-libvirt-run(8)](./man/bcvk-libvirt-run.md)
- [bcvk-libvirt-ssh(8)](./man/bcvk-libvirt-ssh.md)
