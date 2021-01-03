// NOTE: Adapted from cortex-m/build.rs
use std::env;
use std::fs;
use std::io::Result;
use std::path::PathBuf;
use std::process::{Command, ExitStatus, Stdio};

struct Assemble {
    files: Vec<PathBuf>,
    out_dir: PathBuf,
    libname: String,
}

impl Assemble {
    pub fn run(self) -> Result<()> {
        self.detect_changes();
        assert!(self.assemble()?.success());
        assert!(self.archive()?.success());
        Ok(())
    }

    fn detect_changes(&self) {
        for file in &self.files {
            println!("cargo:rerun-if-changed={}", file.display());
        }
    }

    fn o_file(&self) -> PathBuf {
        self.out_dir.join(format!("{}.o", self.libname))
    }

    fn archive_file(&self) -> PathBuf {
        self.out_dir.join(format!("lib{}.a", self.libname))
    }

    fn assemble(&self) -> Result<ExitStatus> {
        let mut cmd = Command::new("arm-none-eabi-as");
        cmd.args(&self.files)
            .args(&["-march=armv6k", "-mfloat-abi=hard"])
            .arg("-o")
            .arg(self.o_file())
            .stdout(Stdio::null());

        eprintln!("Running assembler: {:?}", &cmd);

        cmd.status()
    }

    fn archive(&self) -> Result<ExitStatus> {
        println!("cargo:rustc-link-lib={}", self.libname);

        let mut cmd = Command::new("arm-none-eabi-ar");
        cmd.arg("crs")
            .arg(self.archive_file())
            .arg(self.o_file())
            .stdout(Stdio::null());

        eprintln!("Running archiver: {:?}", &cmd);

        cmd.status()
    }
}

struct LinkerScript {
    script: PathBuf,
    out_dir: PathBuf,
}

impl LinkerScript {
    pub fn include(self) -> Result<()> {
        fs::copy(
            &self.script,
            self.out_dir.join(
                self.script
                    .file_name()
                    .expect("linker script path points to a directory instead of a file"),
            ),
        )?;
        println!("cargo:rustc-link-search={}", self.out_dir.display());
        println!("cargo:rerun-if-changed={}", self.script.display());
        Ok(())
    }
}

fn main() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let libname = env::var("CARGO_PKG_NAME").unwrap();

    // Assemble runtime support objects
    Assemble {
        files: vec!["rsrt0.S".into()],
        out_dir: out_dir.clone(),
        libname,
    }
    .run()
    .expect("Failed to build runtime support objects");

    // Put the linker script somewhere the linker can find it
    LinkerScript {
        script: "link.x".into(),
        out_dir: out_dir,
    }
    .include()
    .expect("Failed to copy linker script");
}
