//! Windows process-tree lifetime guard. This is cleanup containment, not a
//! sandbox: job objects do not block filesystem, network or inherited rights.
//!
//! We spawn the initial thread *suspended* before attaching it to the job.
//! This avoids the ordinary std::process::Command::spawn -> AssignProcessToJob
//! race in which an executable could create an untracked grandchild. The
//! suspended primary thread is located with the documented Toolhelp API and
//! resumed only after successful assignment. Failures kill/reap the child.

use std::{
    io,
    mem::{size_of, zeroed},
    os::windows::{io::AsRawHandle, process::CommandExt},
    process::{Child, Command},
    ptr,
};

use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE},
    System::{
        Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD, THREADENTRY32,
        },
        JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
            SetInformationJobObject, TerminateJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        },
        Threading::{
            OpenThread, ResumeThread, CREATE_NO_WINDOW, CREATE_SUSPENDED, THREAD_SUSPEND_RESUME,
        },
    },
};

struct OwnedHandle(HANDLE);

impl OwnedHandle {
    fn from_win32(value: HANDLE) -> io::Result<Self> {
        if value.is_null() || value == INVALID_HANDLE_VALUE {
            Err(io::Error::last_os_error())
        } else {
            Ok(Self(value))
        }
    }
}

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        // Every job is configured KILL_ON_JOB_CLOSE before creating a child.
        unsafe { CloseHandle(self.0) };
    }
}

pub(super) struct ProcessJob {
    handle: OwnedHandle,
}

impl ProcessJob {
    pub(super) fn create() -> io::Result<Self> {
        let handle =
            OwnedHandle::from_win32(unsafe { CreateJobObjectW(ptr::null(), ptr::null()) })?;
        let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { zeroed() };
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let ok = unsafe {
            SetInformationJobObject(
                handle.0,
                JobObjectExtendedLimitInformation,
                &limits as *const _ as *const _,
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self { handle })
    }

    pub(super) fn spawn_suspended_then_assign(&self, command: &mut Command) -> io::Result<Child> {
        // CommandExt preserves Rust's correct Windows quoting, environment,
        // cwd and stdout/stderr pipe handling. A suspended primary thread
        // cannot create a grandchild before job assignment.
        command.creation_flags(CREATE_SUSPENDED | CREATE_NO_WINDOW);
        let mut child = command.spawn()?;
        let result = (|| {
            if unsafe { AssignProcessToJobObject(self.handle.0, child.as_raw_handle()) } == 0 {
                return Err(io::Error::last_os_error());
            }
            resume_exactly_one_primary_thread(child.id())
        })();
        if let Err(error) = result {
            // In either failure path the primary thread has not run. Once
            // assigned, the job guard also terminates descendants on drop.
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
        Ok(child)
    }

    pub(super) fn terminate(&self) -> io::Result<()> {
        if unsafe { TerminateJobObject(self.handle.0, 1) } == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }
}

fn resume_exactly_one_primary_thread(pid: u32) -> io::Result<()> {
    let snapshot =
        OwnedHandle::from_win32(unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) })?;
    let mut entry: THREADENTRY32 = unsafe { zeroed() };
    entry.dwSize = size_of::<THREADENTRY32>() as u32;
    if unsafe { Thread32First(snapshot.0, &mut entry) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let mut primary_tid = None;
    loop {
        if entry.th32OwnerProcessID == pid && primary_tid.replace(entry.th32ThreadID).is_some() {
            return Err(io::Error::other("suspended child has multiple threads"));
        }
        if unsafe { Thread32Next(snapshot.0, &mut entry) } == 0 {
            break;
        }
    }
    let tid = primary_tid.ok_or_else(|| io::Error::other("suspended primary thread missing"))?;
    let thread = OwnedHandle::from_win32(unsafe { OpenThread(THREAD_SUSPEND_RESUME, 0, tid) })?;
    // 1 is the expected previous suspension count. 0 or >1 indicates an
    // unexpected external modification; caller fails closed by killing job.
    let previous = unsafe { ResumeThread(thread.0) };
    if previous != 1 {
        return Err(io::Error::other("unexpected primary thread suspend state"));
    }
    Ok(())
}
