#!/usr/bin/env bash

# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

# Cargo runner that starts the built ELF in Azahar, or on real hardware via `3dslink-runner.sh`.
#
# Usage: runner.sh <path/to/program.elf> [program arguments...]
#
# Set `CTRU_RT_RUNNER` to pick where the program runs:
#   azahar   (default) in the Azahar emulator
#   3dslink  on a 3DS running the Homebrew Launcher's netloader; `cargo run-3ds` sets this
#
# This indirection exists because cargo resolves a relative `runner` path from `.cargo/config.toml`
# against the directory containing `.cargo`, but one given on the command line (as an alias would)
# against the current directory, which differs between workspace members.

set -euo pipefail

case ${CTRU_RT_RUNNER:-azahar} in
    azahar)
        exec azahar "$@"
        ;;
    3dslink)
        exec "$(dirname "${BASH_SOURCE[0]}")/3dslink-runner.sh" "$@"
        ;;
    *)
        echo "$0: unknown CTRU_RT_RUNNER '$CTRU_RT_RUNNER' (expected 'azahar' or '3dslink')" >&2
        exit 1
        ;;
esac
