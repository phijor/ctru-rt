// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// NOTE: Adapted from cortex-m/build.rs

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Context;
use anyhow::Result;

fn main() -> Result<()> {
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    println!("cargo:rerun-if-changed=build.rs");

    copy_linker_script(&out_dir)?;

    Ok(())
}

/// Put the linker script somewhere the linker can find it
fn copy_linker_script(out_dir: &Path) -> Result<()> {
    let script = PathBuf::from("link.x");

    fs::copy(&script, out_dir.join(script.file_name().unwrap()))
        .context("Failed to copy linker script")?;
    println!("cargo:rerun-if-changed={}", script.display());
    println!("cargo:rustc-link-search={}", out_dir.display());

    Ok(())
}
