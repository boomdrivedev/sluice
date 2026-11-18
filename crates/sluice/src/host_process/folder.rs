//! The host process's folder, `host/`, held open from its start (135;
//! design D1, D6). Every file the process opens, makes or moves there is
//! named through the folder it holds, never through the folder's path, so
//! a folder made again at that path, which is no longer its own, is
//! written nothing whatever the process is doing when it moves: an answer
//! that notes a repository, the idle end taking `start.lock`, a hand-over
//! opening the log. A command's index opens the folder its path names now
//! and does each of its writes there, so a lock and the write under it are
//! in one folder.

use std::fs::File;
use std::io::{ErrorKind, Read, Write};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use nix::fcntl::{openat, renameat, OFlag};
use nix::sys::stat::Mode;

/// A folder held open, and named by the path it was opened at.
#[derive(Debug)]
pub struct Folder {
    file: File,
    path: PathBuf,
    /// Its device and inode, read from the folder held.
    id: (u64, u64),
}

/// How a file in the folder is opened.
#[derive(Clone, Copy)]
pub enum How {
    Read,
    /// Made where it is not there, never emptied: a lock's.
    Lock,
    /// Made where it is not there, and emptied.
    Write,
    /// Made where it is not there, and written at its end.
    Append,
}

impl Folder {
    /// The folder at `path`, held open; an error where it is not there or
    /// is no folder.
    pub fn open(path: &Path) -> std::io::Result<Folder> {
        let file = File::open(path)?;
        let m = file.metadata()?;
        if !m.is_dir() {
            return Err(std::io::Error::new(
                ErrorKind::NotADirectory,
                format!("{} is no folder", path.display()),
            ));
        }
        Ok(Folder {
            file,
            path: path.to_path_buf(),
            id: (m.dev(), m.ino()),
        })
    }

    /// The path it was opened at, which names its files in messages.
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn id(&self) -> (u64, u64) {
        self.id
    }

    /// The file `name` in it, as `how` says, closed on exec as every file
    /// of std's is.
    pub fn file(&self, name: &str, how: How) -> std::io::Result<File> {
        let flags = OFlag::O_CLOEXEC
            | match how {
                How::Read => OFlag::O_RDONLY,
                How::Lock => OFlag::O_WRONLY | OFlag::O_CREAT,
                How::Write => OFlag::O_WRONLY | OFlag::O_CREAT | OFlag::O_TRUNC,
                How::Append => OFlag::O_WRONLY | OFlag::O_CREAT | OFlag::O_APPEND,
            };
        let fd = openat(&self.file, name, flags, Mode::from_bits_truncate(0o666))?;
        Ok(File::from(fd))
    }

    /// The whole of `name`.
    pub fn read(&self, name: &str) -> std::io::Result<Vec<u8>> {
        let mut bytes = Vec::new();
        self.file(name, How::Read)?.read_to_end(&mut bytes)?;
        Ok(bytes)
    }

    /// Write `name` whole: `bytes` into `.<name>.tmp`, moved over it.
    pub fn write(&self, name: &str, bytes: &[u8]) -> Result<()> {
        let tmp = format!(".{name}.tmp");
        self.file(&tmp, How::Write)
            .and_then(|mut f| f.write_all(bytes))
            .with_context(|| format!("writing {}", self.path.join(&tmp).display()))?;
        self.rename(&tmp, name)
    }

    /// Move `from` to `to`, both in it.
    pub fn rename(&self, from: &str, to: &str) -> Result<()> {
        renameat(&self.file, from, &self.file, to)
            .with_context(|| format!("writing {}", self.path.join(to).display()))
    }

    /// The lock on `name` taken now, or `None` where another process holds
    /// it.
    pub fn try_lock(&self, name: &str) -> Result<Option<File>> {
        let path = self.path.join(name);
        let file = self
            .file(name, How::Lock)
            .with_context(|| format!("creating {}", path.display()))?;
        match file.try_lock() {
            Ok(()) => Ok(Some(file)),
            Err(std::fs::TryLockError::WouldBlock) => Ok(None),
            Err(std::fs::TryLockError::Error(e)) => {
                Err(e).with_context(|| format!("locking {}", path.display()))
            }
        }
    }

    /// The lock on `name`, once whatever holds it lets go.
    pub fn lock(&self, name: &str) -> Result<File> {
        let path = self.path.join(name);
        let file = self
            .file(name, How::Lock)
            .with_context(|| format!("creating {}", path.display()))?;
        file.lock()
            .with_context(|| format!("locking {}", path.display()))?;
        Ok(file)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 135 (design D6): a folder held writes into itself once it is moved
    /// and another is made at its path, and the other is written nothing.
    #[test]
    fn a_folder_held_is_written_where_it_went() {
        let base = std::env::temp_dir().join(format!(
            "sluice-folder-{}-{:?}",
            std::process::id(),
            std::time::SystemTime::now()
        ));
        let (dir, aside) = (base.join("host"), base.join("aside"));
        std::fs::create_dir_all(&dir).unwrap();
        let folder = Folder::open(&dir).unwrap();
        folder.write("version", b"1").unwrap();
        std::fs::rename(&dir, &aside).unwrap();
        std::fs::create_dir(&dir).unwrap();
        folder.write("repositories", b"[]").unwrap();
        folder.try_lock("start.lock").unwrap().unwrap();
        folder.file("log", How::Append).unwrap();
        let mut moved: Vec<_> = std::fs::read_dir(&aside)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        moved.sort();
        assert_eq!(moved, ["log", "repositories", "start.lock", "version"]);
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
        assert_eq!(folder.read("version").unwrap(), b"1");
        let _ = std::fs::remove_dir_all(&base);
    }
}
