//! POSIX process-group lifetime cleanup for explicitly approved commands.
//!
//! The child calls `setsid` before exec, so its process ID is the session ID
//! and process-group ID before *any* executable code runs. The parent verifies
//! that identity and never reaps the child until after it has signalled the
//! entire group. An unreaped child keeps its PID reserved, preventing a
//! recycled PGID from targeting an unrelated group on parent-exit cleanup.
//! This is cleanup containment, not a security sandbox or an egress policy.

use std::{
    io, mem,
    os::unix::process::CommandExt,
    process::{Child, Command},
};

use crate::verified_open::WriteParent;

// Darwin's kill(-pgid, SIGKILL) can report EPERM when the session leader is
// exiting. EPERM also means a live descendant may have changed UID, so errno
// alone must NEVER be treated as successful cleanup. Inspect the entire group
// through native libproc in this one narrow case. Apple's proc_listpids also
// returns zero on an empty group *and* on errors: the latter sets errno.
#[cfg(target_os = "macos")]
#[link(name = "proc")]
unsafe extern "C" {
    fn proc_listpids(
        kind: u32,
        type_info: u32,
        buffer: *mut libc::c_void,
        buffer_size: libc::c_int,
    ) -> libc::c_int;
}

#[cfg(target_os = "macos")]
fn only_exited_leader_in_group(
    leader: libc::pid_t,
    exited: bool,
    complete: bool,
    member_pids: &[libc::pid_t],
) -> bool {
    // XNU may detach a zombie from its former pgrp before the parent reaps
    // it. A complete, truly empty census means no living descendant remains.
    leader > 1 && exited && complete && (member_pids.is_empty() || member_pids == [leader])
}

#[cfg(target_os = "macos")]
fn checked_census_count(
    returned_bytes: libc::c_int,
    call_errno: libc::c_int,
    buffer_bytes: usize,
    pid_size: usize,
) -> Option<usize> {
    // libproc proc_listpids() returns 0 on its underlying syscall's error
    // as well as for an empty result. errno must have been cleared just
    // before the call, so a zero return with a nonzero errno is *not* empty.
    // Full buffers may be truncated by XNU: never claim no descendants.
    let bytes = usize::try_from(returned_bytes).ok()?;
    if (bytes == 0 && call_errno != 0)
        || pid_size == 0
        || buffer_bytes < pid_size
        || bytes >= buffer_bytes
        || bytes % pid_size != 0
    {
        return None;
    }
    Some(bytes / pid_size)
}

pub(super) struct UnixProcessGroup {
    pgid: libc::pid_t,
    armed: bool,
}

impl UnixProcessGroup {
    pub(super) fn spawn(
        command: &mut Command,
        pinned_cwd: &WriteParent,
    ) -> io::Result<(Child, Self)> {
        let cwd_fd = pinned_cwd.raw_directory_fd();
        // SAFETY: the pre-exec callback only invokes the async-signal-safe
        // fchdir and setsid syscalls and reads errno if either fails; it does not allocate,
        // lock, spawn threads or access mutable parent application state.
        unsafe {
            command.pre_exec(move || {
                if libc::fchdir(cwd_fd) == -1 {
                    return Err(io::Error::last_os_error());
                }
                if libc::setsid() == -1 {
                    Err(io::Error::last_os_error())
                } else {
                    Ok(())
                }
            });
        }
        let mut child = command.spawn()?;
        let pid = match libc::pid_t::try_from(child.id()) {
            Ok(pid) if pid > 1 => pid,
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(io::Error::other("unsafe child PID for process group"));
            }
        };

        // `setsid` succeeded if Command::spawn returned Ok. Hold the child
        // unreaped even when validation fails, so the negative PID is never
        // recycled while a best-effort group cleanup runs.
        let mut group = Self {
            pgid: pid,
            armed: true,
        };
        let actual = unsafe { libc::getpgid(pid) };
        let our_pgid = unsafe { libc::getpgrp() };
        if actual != pid || our_pgid == pid {
            let _ = group.terminate();
            let _ = child.kill();
            // In a failure case `terminate` can leave the group guard armed.
            // Drop it *before* wait/reap, while this PID cannot be recycled.
            drop(group);
            let _ = child.wait();
            return Err(io::Error::other("child did not join its own process group"));
        }
        Ok((child, group))
    }

    /// Inspect exit without reaping the leader. Calling `Child::try_wait`
    /// before kill(-pgid) would free that PID and open a group-ID reuse race.
    pub(super) fn parent_exited_without_reaping(&self) -> io::Result<bool> {
        // SAFETY: waitid operates on our own unreaped child, and this zeroed
        // siginfo_t is initialized by the kernel before it is inspected.
        let mut info: libc::siginfo_t = unsafe { mem::zeroed() };
        loop {
            let status = unsafe {
                libc::waitid(
                    libc::P_PID,
                    self.pgid as libc::id_t,
                    &mut info,
                    libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
                )
            };
            if status == 0 {
                // SAFETY: libc's platform accessor is defined on siginfo_t
                // for both Linux and macOS. With WNOHANG, 0 means no exit.
                let reported_pid = unsafe { info.si_pid() };
                if reported_pid != 0 && reported_pid != self.pgid {
                    return Err(io::Error::other("waitid returned a different child"));
                }
                return Ok(reported_pid == self.pgid);
            }
            let error = io::Error::last_os_error();
            if error.kind() != io::ErrorKind::Interrupted {
                return Err(error);
            }
        }
    }

    pub(super) fn terminate(&mut self) -> io::Result<()> {
        if !self.armed {
            return Ok(());
        }
        if self.pgid <= 1 || unsafe { libc::getpgrp() } == self.pgid {
            return Err(io::Error::other("refusing to signal host process group"));
        }
        // SAFETY: `setsid` made the child leader with pgid == pid before
        // exec; no code path reaps it before this signal, so the PGID cannot
        // have been reused for an unrelated user's shell or process group.
        let status = unsafe { libc::kill(-self.pgid, libc::SIGKILL) };
        if status != 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() != Some(libc::ESRCH) {
                #[cfg(target_os = "macos")]
                if error.raw_os_error() == Some(libc::EPERM)
                    && self.macos_only_unreaped_zombie_leader()?
                {
                    // An exited, unreaped leader is the sole remaining group
                    // member. Its PID is still reserved, and there is no
                    // living member that could spawn a new descendant.
                    self.armed = false;
                    return Ok(());
                }
                return Err(error);
            }
        }
        self.armed = false;
        Ok(())
    }

    #[cfg(target_os = "macos")]
    fn macos_only_unreaped_zombie_leader(&self) -> io::Result<bool> {
        // Confirm the leader has exited WITHOUT wait/reap: reaping before
        // signalling the group allows PID/PGID reuse and unrelated kills.
        if !self.parent_exited_without_reaping()? {
            return Ok(false);
        }

        // PROC_PGRP_ONLY = 2 (Apple libproc.h). XNU's NULL-buffer size query
        // returns (nprocs + 20) * sizeof(int), EVEN for an empty group; it
        // must be positive. A zero estimate is thus an error, not evidence of
        // emptiness, and must remain fail-closed. Only the filled-buffer call
        // can report a successful zero-member census. Reserve additional
        // slots for concurrent changes and reject full/truncated results.
        const PROC_PGRP_ONLY: u32 = 2;
        const EXTRA_SLOTS: usize = 32;
        const MAX_CENSUS_BYTES: usize = 1 << 20;
        let estimated =
            unsafe { proc_listpids(PROC_PGRP_ONLY, self.pgid as u32, std::ptr::null_mut(), 0) };
        if estimated <= 0 {
            return Ok(false);
        }
        let pid_size = mem::size_of::<libc::pid_t>();
        let Some(buffer_bytes) = usize::try_from(estimated)
            .ok()
            .and_then(|n| n.checked_add(EXTRA_SLOTS * pid_size))
            .filter(|&n| n <= MAX_CENSUS_BYTES)
        else {
            return Ok(false);
        };
        let mut members = vec![0 as libc::pid_t; buffer_bytes.div_ceil(pid_size)];
        let capacity = members.len() * pid_size;
        // SAFETY: Darwin's thread-local __error() points at this thread's
        // errno. Clearing it immediately before the libproc wrapper call is
        // necessary because that wrapper maps syscall failure -1 to 0.
        unsafe { *libc::__error() = 0 };
        let count_bytes = unsafe {
            proc_listpids(
                PROC_PGRP_ONLY,
                self.pgid as u32,
                members.as_mut_ptr().cast(),
                capacity as libc::c_int,
            )
        };
        let call_errno = unsafe { *libc::__error() };
        let Some(member_count) = checked_census_count(count_bytes, call_errno, capacity, pid_size)
        else {
            return Ok(false);
        };
        members.truncate(member_count);
        Ok(only_exited_leader_in_group(
            self.pgid,
            self.parent_exited_without_reaping()?,
            true,
            &members,
        ))
    }
}

impl Drop for UnixProcessGroup {
    fn drop(&mut self) {
        // On errors after spawn (including pipe/thread/waitid errors), the
        // guard kills the group while the direct-child PID is still reserved.
        let _ = self.terminate();
    }
}

#[cfg(test)]
mod cwd_identity_tests {
    use super::UnixProcessGroup;
    use crate::verified_open::pin_parent;
    use std::{
        fs,
        io::Read,
        path::Path,
        process::{Command, Stdio},
    };

    #[test]
    fn child_uses_open_directory_object_after_rename_and_path_replacement() {
        let workspace = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(workspace.path()).unwrap();
        let cwd = root.join("approved");
        let moved = root.join("renamed");
        fs::create_dir(&cwd).unwrap();
        let guard = pin_parent(&root, Path::new("approved/.constellation-cwd-pin")).unwrap();
        fs::rename(&cwd, &moved).unwrap();
        fs::create_dir(&cwd).unwrap();

        let mut command = Command::new("/bin/pwd");
        command.stdout(Stdio::piped()).stderr(Stdio::null());
        let (mut child, mut group) = UnixProcessGroup::spawn(&mut command, &guard).unwrap();
        let mut output = String::new();
        child
            .stdout
            .take()
            .unwrap()
            .read_to_string(&mut output)
            .unwrap();
        group.terminate().unwrap();
        let status = child.wait().unwrap();
        assert!(status.success());
        assert_eq!(output.trim_end(), moved.to_string_lossy());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn zombie_only_exemption_rejects_every_ambiguous_or_live_group() {
        use super::only_exited_leader_in_group;
        let leader = 4137;
        assert!(only_exited_leader_in_group(leader, true, true, &[leader]));
        assert!(only_exited_leader_in_group(leader, true, true, &[]));
        for (exited, complete, members) in [
            (false, true, vec![leader]),
            (false, true, vec![]),
            (true, false, vec![leader]),
            (true, false, vec![]),
            (true, true, vec![leader, 4138]),
            (true, true, vec![4138]),
            (true, true, vec![leader, leader]),
        ] {
            assert!(!only_exited_leader_in_group(
                leader, exited, complete, &members
            ));
        }
        assert!(!only_exited_leader_in_group(1, true, true, &[1]));
        assert!(!only_exited_leader_in_group(1, true, true, &[]));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn libproc_zero_is_empty_only_with_clean_errno_and_complete_buffer() {
        use super::checked_census_count;
        // The native wrapper maps syscall errors to zero: never mistake
        // EPERM/EACCES/EINVAL/ENOMEM for a fully observed empty group.
        assert_eq!(checked_census_count(0, 0, 256, 4), Some(0));
        for errno in [libc::EPERM, libc::EACCES, libc::EINVAL, libc::ENOMEM] {
            assert_eq!(checked_census_count(0, errno, 256, 4), None);
        }
        assert_eq!(checked_census_count(-1, 0, 256, 4), None);
        assert_eq!(checked_census_count(1, 0, 256, 4), None);
        assert_eq!(checked_census_count(6, 0, 256, 4), None);
        assert_eq!(checked_census_count(256, 0, 256, 4), None);
        assert_eq!(checked_census_count(260, 0, 256, 4), None);
        assert_eq!(checked_census_count(0, 0, 0, 4), None);
        assert_eq!(checked_census_count(0, 0, 256, 0), None);
        assert_eq!(checked_census_count(4, 0, 256, 4), Some(1));
        assert_eq!(checked_census_count(8, 0, 256, 4), Some(2));
        // A stale errno after a positive byte count is not a libproc error;
        // only the zero return is ambiguous in libproc's documented wrapper.
        assert_eq!(checked_census_count(4, libc::EPERM, 256, 4), Some(1));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn repeated_exited_unreaped_leaders_do_not_fail_on_empty_group_census() {
        // Repeated short-lived children increase coverage for Darwin's race
        // between exit, zombie-list membership and process-group teardown.
        // Each child remains unreaped until after group cleanup is checked.
        let workspace = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(workspace.path()).unwrap();
        let pinned = root.join("approved");
        fs::create_dir(&pinned).unwrap();
        let guard = pin_parent(&root, Path::new("approved/.constellation-cwd-pin")).unwrap();
        for case in 0..16 {
            // macOS runners have /bin/sh but need not have Linux's /bin/true.
            // Exit in the child without depending on PATH or a system utility.
            let mut command = Command::new("/bin/sh");
            command.args(["-c", "exit 0"]);
            command.stdout(Stdio::null()).stderr(Stdio::null());
            let (mut child, mut group) = UnixProcessGroup::spawn(&mut command, &guard).unwrap();
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
            while !group.parent_exited_without_reaping().unwrap() {
                assert!(
                    std::time::Instant::now() < deadline,
                    "Darwin child {case} did not exit before cleanup"
                );
                std::thread::yield_now();
            }
            group.terminate().unwrap_or_else(|error| {
                panic!("unreaped Darwin exit-case {case} cleanup failed: {error}")
            });
            assert!(child.wait().unwrap().success());
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn live_leader_with_background_descendant_cannot_use_exited_group_exemption() {
        use std::io::BufRead;

        let workspace = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(workspace.path()).unwrap();
        fs::create_dir(root.join("approved")).unwrap();
        let guard = pin_parent(&root, Path::new("approved/.constellation-cwd-pin")).unwrap();

        // The shell launches a background descendant before signaling ready.
        // Keep its leader alive with wait, then verify the zombie-only path
        // rejects the *live* process group before the ordinary group kill.
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "sleep 30 & printf 'ready\\n'; wait"]);
        command.stdout(Stdio::piped()).stderr(Stdio::null());
        let (mut child, mut group) = UnixProcessGroup::spawn(&mut command, &guard).unwrap();
        let mut line = String::new();
        std::io::BufReader::new(child.stdout.take().unwrap())
            .read_line(&mut line)
            .unwrap();
        assert_eq!(line, "ready\n");
        assert!(!group.parent_exited_without_reaping().unwrap());
        assert!(!group.macos_only_unreaped_zombie_leader().unwrap());
        group.terminate().unwrap();
        assert!(!child.wait().unwrap().success());
    }
}
