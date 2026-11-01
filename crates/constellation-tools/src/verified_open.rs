//! Open read handles against the exact approved workspace path. Check the
//! handle itself before reading bytes; pathname inspection alone races swaps.

use std::{fs::File, io::Write, path::Path};

use crate::{Result, ToolError};

/// The identity of a directory object, independent of its mutable pathname.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct DirectoryIdentity {
    volume: u64,
    file: u64,
}

/// Keep the directory that owns a write target open until publication. On
/// Windows, also keep every ancestor open without FILE_SHARE_DELETE, so a
/// concurrent rename/junction swap cannot redirect pathname-based publication.
pub(crate) struct WriteParent {
    identity: DirectoryIdentity,
    #[cfg(unix)]
    handle: std::os::fd::OwnedFd,
    #[cfg(windows)]
    _ancestors: Vec<File>,
}

pub(crate) struct StagedFile {
    path: std::path::PathBuf,
    // On Windows the handle excludes FILE_SHARE_DELETE, so another process
    // cannot replace a staged create-new source before it is hardlinked.
    handle: Option<File>,
}

impl WriteParent {
    pub(crate) fn identity(&self) -> DirectoryIdentity {
        self.identity
    }

    #[cfg(unix)]
    pub(crate) fn raw_directory_fd(&self) -> std::os::fd::RawFd {
        use std::os::fd::AsRawFd;
        self.handle.as_raw_fd()
    }

    /// Stage entirely in the already opened directory. A unique, exclusive
    /// name prevents clobbering an existing workspace entry.
    pub(crate) fn stage(&self, target: &Path, bytes: &[u8]) -> Result<StagedFile> {
        let staged = target.with_extension(format!("constellation-{}.tmp", uuid::Uuid::new_v4()));
        #[cfg(unix)]
        let mut handle = {
            use std::{
                ffi::CString,
                os::{
                    fd::{AsRawFd, FromRawFd},
                    unix::ffi::OsStrExt,
                },
            };
            let name = CString::new(
                staged
                    .file_name()
                    .ok_or(ToolError::InvalidRelativePath)?
                    .as_bytes(),
            )
            .map_err(|_| ToolError::InvalidRelativePath)?;
            let fd = unsafe {
                libc::openat(
                    self.handle.as_raw_fd(),
                    name.as_ptr(),
                    libc::O_CREAT
                        | libc::O_EXCL
                        | libc::O_NOFOLLOW
                        | libc::O_CLOEXEC
                        | libc::O_WRONLY,
                    0o600,
                )
            };
            if fd < 0 {
                return Err(ToolError::Io(std::io::Error::last_os_error()));
            }
            unsafe { File::from_raw_fd(fd) }
        };
        #[cfg(windows)]
        let mut handle = {
            use std::os::windows::fs::OpenOptionsExt;
            // Retain the file handle until the write and sync complete.
            std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .share_mode(
                    windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ
                        | windows_sys::Win32::Storage::FileSystem::FILE_SHARE_WRITE,
                )
                .open(&staged)?
        };
        #[cfg(not(any(unix, windows)))]
        return Err(ToolError::ScopeEscape);
        let output = handle.write_all(bytes).and_then(|()| handle.sync_all());
        if let Err(error) = output {
            drop(handle);
            let _ = self.cleanup(&mut StagedFile {
                path: staged,
                handle: None,
            });
            return Err(ToolError::Io(error));
        }
        Ok(StagedFile {
            path: staged,
            handle: Some(handle),
        })
    }

    pub(crate) fn publish_new(&self, staged: &StagedFile, target: &Path) -> Result<()> {
        let staged = staged.path.as_path();
        #[cfg(unix)]
        {
            use std::{
                ffi::CString,
                os::{fd::AsRawFd, unix::ffi::OsStrExt},
            };
            let source = CString::new(
                staged
                    .file_name()
                    .ok_or(ToolError::InvalidRelativePath)?
                    .as_bytes(),
            )
            .map_err(|_| ToolError::InvalidRelativePath)?;
            let dest = CString::new(
                target
                    .file_name()
                    .ok_or(ToolError::InvalidRelativePath)?
                    .as_bytes(),
            )
            .map_err(|_| ToolError::InvalidRelativePath)?;
            let result = unsafe {
                libc::linkat(
                    self.handle.as_raw_fd(),
                    source.as_ptr(),
                    self.handle.as_raw_fd(),
                    dest.as_ptr(),
                    0,
                )
            };
            if result < 0 {
                let error = std::io::Error::last_os_error();
                return Err(if error.kind() == std::io::ErrorKind::AlreadyExists {
                    ToolError::FileAlreadyExists
                } else {
                    ToolError::Io(error)
                });
            }
            Ok(())
        }
        #[cfg(windows)]
        {
            std::fs::hard_link(staged, target).map_err(|error| {
                if error.kind() == std::io::ErrorKind::AlreadyExists {
                    ToolError::FileAlreadyExists
                } else {
                    ToolError::Io(error)
                }
            })
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = (staged, target);
            Err(ToolError::ScopeEscape)
        }
    }

    pub(crate) fn publish_replace(&self, staged: &mut StagedFile, target: &Path) -> Result<()> {
        // Replacement requires renaming the staging inode. Windows forbids
        // this while our staged handle denies FILE_SHARE_DELETE.
        drop(staged.handle.take());
        let staged = staged.path.as_path();
        #[cfg(unix)]
        {
            use std::{
                ffi::CString,
                os::{fd::AsRawFd, unix::ffi::OsStrExt},
            };
            let source = CString::new(
                staged
                    .file_name()
                    .ok_or(ToolError::InvalidRelativePath)?
                    .as_bytes(),
            )
            .map_err(|_| ToolError::InvalidRelativePath)?;
            let dest = CString::new(
                target
                    .file_name()
                    .ok_or(ToolError::InvalidRelativePath)?
                    .as_bytes(),
            )
            .map_err(|_| ToolError::InvalidRelativePath)?;
            let result = unsafe {
                libc::renameat(
                    self.handle.as_raw_fd(),
                    source.as_ptr(),
                    self.handle.as_raw_fd(),
                    dest.as_ptr(),
                )
            };
            if result < 0 {
                return Err(ToolError::Io(std::io::Error::last_os_error()));
            }
            Ok(())
        }
        #[cfg(windows)]
        {
            super::execution::replace_file(staged, target)
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = (staged, target);
            Err(ToolError::ScopeEscape)
        }
    }

    pub(crate) fn cleanup(&self, staged: &mut StagedFile) -> Result<()> {
        drop(staged.handle.take());
        let staged = staged.path.as_path();
        #[cfg(unix)]
        {
            use std::{
                ffi::CString,
                os::{fd::AsRawFd, unix::ffi::OsStrExt},
            };
            let name = CString::new(
                staged
                    .file_name()
                    .ok_or(ToolError::InvalidRelativePath)?
                    .as_bytes(),
            )
            .map_err(|_| ToolError::InvalidRelativePath)?;
            let result = unsafe { libc::unlinkat(self.handle.as_raw_fd(), name.as_ptr(), 0) };
            if result < 0 {
                return Err(ToolError::Io(std::io::Error::last_os_error()));
            }
            Ok(())
        }
        #[cfg(windows)]
        {
            Ok(std::fs::remove_file(staged)?)
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = staged;
            Err(ToolError::ScopeEscape)
        }
    }
}

#[cfg(unix)]
pub(crate) fn pin_parent(root: &Path, relative: &Path) -> Result<WriteParent> {
    use std::{
        ffi::CString,
        os::{
            fd::{AsRawFd, FromRawFd, OwnedFd},
            unix::ffi::OsStrExt,
        },
        path::Component,
    };
    let root_name =
        CString::new(root.as_os_str().as_bytes()).map_err(|_| ToolError::InvalidRelativePath)?;
    let root_fd = unsafe {
        libc::open(
            root_name.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if root_fd < 0 {
        return Err(ToolError::Io(std::io::Error::last_os_error()));
    }
    let mut handle = unsafe { OwnedFd::from_raw_fd(root_fd) };
    let parent = relative.parent().unwrap_or(Path::new(""));
    for component in parent.components() {
        let Component::Normal(segment) = component else {
            return Err(ToolError::InvalidRelativePath);
        };
        let name = CString::new(segment.as_bytes()).map_err(|_| ToolError::InvalidRelativePath)?;
        let fd = unsafe {
            libc::openat(
                handle.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if fd < 0 {
            return Err(ToolError::Io(std::io::Error::last_os_error()));
        }
        handle = unsafe { OwnedFd::from_raw_fd(fd) };
    }
    use std::os::unix::fs::MetadataExt;
    let metadata = File::from(handle.try_clone()?).metadata()?;
    Ok(WriteParent {
        identity: DirectoryIdentity {
            volume: metadata.dev(),
            file: metadata.ino(),
        },
        handle,
    })
}

#[cfg(windows)]
pub(crate) fn pin_parent(root: &Path, relative: &Path) -> Result<WriteParent> {
    use std::{
        mem::MaybeUninit,
        os::windows::{fs::OpenOptionsExt, io::AsRawHandle},
        path::Component,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        GetFileInformationByHandle, GetFinalPathNameByHandleW, BY_HANDLE_FILE_INFORMATION,
        FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ,
        FILE_SHARE_WRITE,
    };

    let mut ancestors = Vec::new();
    let mut current = root.to_path_buf();
    let components = std::iter::once(None).chain(
        relative
            .parent()
            .into_iter()
            .flat_map(|p| p.components().map(Some)),
    );
    for component in components {
        if let Some(component) = component {
            let Component::Normal(name) = component else {
                return Err(ToolError::InvalidRelativePath);
            };
            current.push(name);
        }
        let directory = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(&current)?;
        let metadata = directory.metadata()?;
        if !metadata.is_dir() || super::execution::is_reparse(&metadata) {
            return Err(ToolError::SymlinkDenied);
        }
        let mut final_path = vec![0u16; 32_768];
        let length = unsafe {
            GetFinalPathNameByHandleW(
                directory.as_raw_handle(),
                final_path.as_mut_ptr(),
                final_path.len() as u32,
                0,
            )
        };
        if length == 0 || length as usize >= final_path.len() {
            return Err(ToolError::ScopeEscape);
        }
        let opened = String::from_utf16(&final_path[..length as usize])
            .map_err(|_| ToolError::ScopeEscape)?;
        if !opened.eq_ignore_ascii_case(&current.to_string_lossy()) {
            return Err(ToolError::ScopeEscape);
        }
        ancestors.push(directory);
    }
    let mut info = MaybeUninit::<BY_HANDLE_FILE_INFORMATION>::zeroed();
    let ok = unsafe {
        GetFileInformationByHandle(ancestors.last().unwrap().as_raw_handle(), info.as_mut_ptr())
    };
    if ok == 0 {
        return Err(ToolError::Io(std::io::Error::last_os_error()));
    }
    let info = unsafe { info.assume_init() };
    Ok(WriteParent {
        identity: DirectoryIdentity {
            volume: info.dwVolumeSerialNumber as u64,
            file: (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow),
        },
        _ancestors: ancestors,
    })
}

#[cfg(not(any(unix, windows)))]
pub(crate) fn pin_parent(_root: &Path, _relative: &Path) -> Result<WriteParent> {
    Err(ToolError::ScopeEscape)
}

#[cfg(unix)]
pub(crate) fn read(root: &Path, relative: &Path, _approved: &Path) -> Result<File> {
    use std::{
        ffi::CString,
        os::{
            fd::{AsRawFd, FromRawFd, OwnedFd},
            unix::ffi::OsStrExt,
        },
        path::Component,
    };

    fn descriptor(
        parent: Option<&OwnedFd>,
        name: &std::ffi::CStr,
        directory: bool,
    ) -> Result<OwnedFd> {
        let flags = libc::O_RDONLY
            | libc::O_CLOEXEC
            | libc::O_NOFOLLOW
            | if directory { libc::O_DIRECTORY } else { 0 };
        let raw = unsafe {
            match parent {
                Some(parent) => libc::openat(parent.as_raw_fd(), name.as_ptr(), flags),
                None => libc::open(name.as_ptr(), flags),
            }
        };
        if raw < 0 {
            return Err(ToolError::Io(std::io::Error::last_os_error()));
        }
        Ok(unsafe { OwnedFd::from_raw_fd(raw) })
    }

    let root_name =
        CString::new(root.as_os_str().as_bytes()).map_err(|_| ToolError::InvalidRelativePath)?;
    let mut directory = descriptor(None, &root_name, true)?;
    let mut components = relative.components().peekable();
    while let Some(component) = components.next() {
        let Component::Normal(name) = component else {
            return Err(ToolError::InvalidRelativePath);
        };
        let segment = CString::new(name.as_bytes()).map_err(|_| ToolError::InvalidRelativePath)?;
        let last = components.peek().is_none();
        let handle = descriptor(Some(&directory), &segment, !last)?;
        if last {
            let file = File::from(handle);
            if !file.metadata()?.is_file() {
                return Err(ToolError::NotRegularFile);
            }
            return Ok(file);
        }
        directory = handle;
    }
    Err(ToolError::InvalidRelativePath)
}

#[cfg(windows)]
pub(crate) fn read(root: &Path, relative: &Path, approved: &Path) -> Result<File> {
    use std::os::{
        raw::c_void,
        windows::{fs::OpenOptionsExt, io::AsRawHandle},
    };
    use windows_sys::Win32::Storage::FileSystem::{
        GetFinalPathNameByHandleW, FILE_FLAG_OPEN_REPARSE_POINT,
    };

    // OPEN_REPARSE_POINT refuses to follow a swapped leaf symlink. A directory
    // renamed to an external junction between inspection and open is detected
    // by checking the actual opened handle's final absolute path.
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(root.join(relative))?;
    if super::execution::is_reparse(&file.metadata()?) {
        return Err(ToolError::SymlinkDenied);
    }
    if !file.metadata()?.is_file() {
        return Err(ToolError::NotRegularFile);
    }
    let mut final_path = vec![0u16; 32_768];
    let length = unsafe {
        GetFinalPathNameByHandleW(
            file.as_raw_handle() as *mut c_void,
            final_path.as_mut_ptr(),
            final_path.len() as u32,
            0,
        )
    };
    if length == 0 || length as usize >= final_path.len() {
        return Err(ToolError::ScopeEscape);
    }
    let opened =
        String::from_utf16(&final_path[..length as usize]).map_err(|_| ToolError::ScopeEscape)?;
    // Both GetFinalPathNameByHandleW and std::fs::canonicalize on Windows use
    // extended-length DOS paths. An unrecognized device path fails closed.
    if !opened.eq_ignore_ascii_case(&approved.to_string_lossy()) {
        return Err(ToolError::ScopeEscape);
    }
    Ok(file)
}

#[cfg(not(any(unix, windows)))]
pub(crate) fn read(_root: &Path, _relative: &Path, _approved: &Path) -> Result<File> {
    Err(ToolError::ScopeEscape)
}

#[cfg(all(test, windows))]
mod windows_write_guard_tests {
    use super::pin_parent;
    use std::{fs, path::Path};

    #[test]
    fn pinned_ancestors_cannot_be_renamed_until_the_guard_is_released() {
        let workspace = tempfile::tempdir().unwrap();
        let parent = workspace.path().join("approved");
        let moved = workspace.path().join("moved");
        fs::create_dir(&parent).unwrap();
        let canonical_root = fs::canonicalize(workspace.path()).unwrap();
        let guard = pin_parent(&canonical_root, Path::new("approved/document.md")).unwrap();
        assert!(fs::rename(&parent, &moved).is_err());
        assert!(parent.is_dir());
        drop(guard);
        fs::rename(&parent, &moved).unwrap();
    }

    #[test]
    fn process_cwd_pin_holds_full_directory_chain_during_spawn() {
        let workspace = tempfile::tempdir().unwrap();
        let parent = workspace.path().join("parent");
        let cwd = parent.join("child");
        fs::create_dir_all(&cwd).unwrap();
        let root = fs::canonicalize(workspace.path()).unwrap();
        let pin = pin_parent(&root, Path::new("parent/child/.constellation-cwd-pin")).unwrap();
        assert!(fs::rename(&cwd, parent.join("swapped-child")).is_err());
        assert!(fs::rename(&parent, workspace.path().join("swapped-parent")).is_err());
        drop(pin);
        fs::rename(&cwd, parent.join("swapped-child")).unwrap();
    }
}
