use std::{env, fs, path::PathBuf};

use color_eyre::eyre::{OptionExt, Result, WrapErr, eyre};

#[cfg_attr(not(feature = "build-logging"), allow(unused))]
macro_rules! warning {
    ($($arg:tt)*) => {
        println!("cargo:warning={}", format!($($arg)*))
    }
}

macro_rules! build_log {
    ($($arg:tt)*) => {
        #[cfg(feature = "build-logging")]
        warning!($($arg)*)
    }
}

fn main() -> Result<()> {
    build_log!("Build script running...");
    println!("cargo:rerun-if-changed=");

    let out_dir_var = env::var_os("OUT_DIR").ok_or_eyre("Failed to get OUT_DIR")?;
    let out_dir = PathBuf::from(&out_dir_var);

    build_log!("Out directory: {out_dir:?}");

    let cargo_manifest_dir = PathBuf::new().join(env::var("CARGO_MANIFEST_DIR")?);
    build_log!("Cargo manifest dir: {cargo_manifest_dir:?}");

    let cfg_generation_file = &out_dir.join("generated_config.kdl");
    let config_template_path = &cargo_manifest_dir.join("template_config.kdl");

    if !config_template_path.exists() {
        build_log!("Config path ({config_template_path:?}) does not exist");
        return Err(eyre!("Configuration template does not exist"));
    }

    build_log!("Configuration path exists ({config_template_path:?})");

    if !config_template_path.is_file() {
        build_log!("Configuration template should be a file");
        return Err(eyre!("Invalid configuration template"));
    }

    fs::copy(config_template_path, cfg_generation_file)
        .wrap_err("Failed to copy configuration template")?;

    build_log!("Default configuration has been copied to {cfg_generation_file:?}");

    Ok(())
}
