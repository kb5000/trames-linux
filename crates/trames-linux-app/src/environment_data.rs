// SPDX-License-Identifier: GPL-3.0-or-later
//! Data operations share the portable Runtime registry/storage implementation.
use std::path::Path;
use trames_distro::{environment::Result, registry::Registry, registry_data as data};
pub fn dispatch(r: &Registry, op: &str, args: &[String]) -> Result<()> {
    if args.len() != 2 {
        return Err(format!("env {op} requires NAME and destination/source").into());
    }
    match op {
        "backup" | "export" => data::backup(r, &args[0], Path::new(&args[1])),
        "restore" => data::restore(
            r,
            &args[0],
            Path::new(&args[1]),
            crate::environments::validate,
        ),
        "import-store" => data::import_store(
            r,
            &args[0],
            Path::new(&args[1]),
            crate::environments::validate,
        ),
        "clone" => data::clone_environment(r, &args[0], &args[1], crate::environments::validate),
        _ => Err("unknown data operation".into()),
    }
}
