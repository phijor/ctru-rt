#!/usr/bin/env bash

# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

# Cargo runner that converts the built ELF to a 3DSX and sends it to a 3DS running the Homebrew
# Launcher's netloader (press Y in the launcher).
#
# Usage: 3dslink-runner.sh <path/to/program.elf> [program arguments...]
#
# Set `THREEDS_ADDRESS` to the 3DS's hostname or IP address to skip the network broadcast.

set -euo pipefail

if [[ $# -lt 1 ]]; then
    echo "usage: $0 <program.elf> [args...]" >&2
    exit 1
fi

elf=$1
shift

# Prefer the tools on $PATH, fall back to the devkitPro installation.
tools=${DEVKITPRO:-/opt/devkitpro}/tools/bin
dsxtool=$(command -v 3dsxtool || echo "$tools/3dsxtool")
dslink=$(command -v 3dslink || echo "$tools/3dslink")

# Put the 3DSX next to the ELF, e.g. `target/.../examples/gsp.3dsx`.
dsx=${elf%.elf}.3dsx
"$dsxtool" "$elf" "$dsx"

link_args=()
if [[ -n ${THREEDS_ADDRESS:-} ]]; then
    link_args+=(--address "$THREEDS_ADDRESS")
fi

exec "$dslink" "${link_args[@]}" "$dsx" "$@"
