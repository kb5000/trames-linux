// SPDX-License-Identifier: GPL-3.0-or-later
//! Cold private-store lifecycle; guest resources retain the shared VFS semantics.
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
const INITIALIZING: &str = ".trames-importing";
struct NewStore {
    path: PathBuf,
    published: bool,
}
impl Drop for NewStore {
    fn drop(&mut self) {
        if !self.published {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}
pub fn open(path: &Path, source: Option<&Path>) -> Result<trames_vfs::Vfs> {
    if let Some(source) = source {
        if !source.exists() {
            return Err("rootfs source does not exist".into());
        }
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            fs::create_dir_all(parent)?;
        }
        if source.is_dir()
            && path
                .parent()
                .unwrap_or(Path::new("."))
                .canonicalize()?
                .join(path.file_name().ok_or("store requires a directory name")?)
                .starts_with(source.canonicalize()?)
        {
            return Err("store must be outside its rootfs source".into());
        }
        // Atomic create-only admission; never replace an existing environment.
        fs::create_dir(path)?;
        let mut created = NewStore {
            path: path.to_owned(),
            published: false,
        };
        let marker = path.join(INITIALIZING);
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&marker)?
            .sync_all()?;
        let mut vfs = trames_vfs::Vfs::open_disk(path.join("fs"), Default::default())?;
        let sha = vfs.populate_staged(|vfs| trames_distro::rootfs::install(source, vfs, None))?;
        let mut provenance = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path.join("rootfs.json"))?;
        writeln!(
            provenance,
            "{}",
            serde_json::json!({"version":1,"sha256":sha})
        )?;
        provenance.sync_all()?;
        vfs.sync()?;
        fs::remove_file(marker)?;
        created.published = true;
        File::open(path)?.sync_all()?;
        // Image import may replace mountpoint inodes. Start from the same
        // freshly reconstructed pseudo mounts as every later store reopen.
        drop(vfs);
        Ok(trames_vfs::Vfs::open_disk(
            path.join("fs"),
            Default::default(),
        )?)
    } else {
        if path.join(INITIALIZING).exists()
            || !path.join("fs/manifest").is_file()
            || !path.join("rootfs.json").is_file()
        {
            return Err("store is absent or its import is incomplete; create a new store with --store DIR --rootfs DIR_OR_ARCHIVE".into());
        }
        let vfs = trames_vfs::Vfs::open_disk(path.join("fs"), Default::default())?;
        // Recheck after taking the original store's single-writer lease.
        if path.join(INITIALIZING).exists() {
            return Err("store import is incomplete".into());
        }
        Ok(vfs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn import_is_create_only_and_owned_files_survive_reopen() {
        let base = std::env::temp_dir().join(format!("trames-linux-store-{}", std::process::id()));
        fs::create_dir(&base).unwrap();
        let source = base.join("source");
        fs::create_dir(&source).unwrap();
        fs::write(source.join("original"), b"initial").unwrap();
        let store = base.join("store");
        let mut vfs = open(&store, Some(&source)).unwrap();
        vfs.import_file(b"/persisted", b"persisted data".to_vec())
            .unwrap();
        vfs.sync().unwrap();
        drop(vfs);
        assert!(open(&store, Some(&source)).is_err());
        let mut reopened = open(&store, None).unwrap();
        let fd = reopened
            .open_at(trames_vfs::AT_FDCWD, b"/persisted", 0, 0)
            .unwrap();
        assert_eq!(reopened.read(fd, 64).unwrap(), b"persisted data");
        drop(reopened);
        let incomplete = base.join("incomplete");
        fs::create_dir(&incomplete).unwrap();
        fs::write(incomplete.join(INITIALIZING), b"").unwrap();
        assert!(open(&incomplete, None).is_err());
        let invalid = base.join("invalid");
        let bogus = base.join("not-an-archive");
        fs::write(&bogus, b"invalid archive").unwrap();
        assert!(open(&invalid, Some(&bogus)).is_err());
        assert!(!invalid.exists());
        fs::remove_dir_all(base).unwrap();
    }
}
