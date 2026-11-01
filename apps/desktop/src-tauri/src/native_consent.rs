//! Independent Windows-native consent for a single, already prepared local
//! file action. Content is reviewed in an OS-native scrollable window.
//!
//! INTEGRATION CONTRACT:
//! - Construct `NativeConsentRequest` inside trusted Rust from the *pending
//!   immutable PreparedOperation*, its saved OperationContext, and preview
//!   details. NEVER reconstruct any field from an IPC approval payload.
//! - Obtain the owner HWND from the current Tauri window in native code; never
//!   accept it from a renderer. A null owner is refused.
//! - On Ok(None) or Err, durably deny/consume the pending operation. On
//!   Ok(Some(proof)), verify its operation ID and digest, recheck task
//!   identity/state/revision under the coordinated dispatch gate, then grant
//!   exactly the pending prepared scope.
//! - Show the dialog outside the tool-host mutex, on an appropriate UI
//!   thread/worker boundary. Do not hold state locks while waiting for input.
//! - This module never starts the application, logs request data, or grants
//!   permission. It does not solve TOCTOU, rollback, task cancellation races,
//!   SendInput/desktop automation, or compromised local OS/user accounts.

use sha2::{Digest, Sha256};
use std::{error::Error, fmt, fmt::Write as _, path::Path};

/// The only action families currently exposed by the local file tool host.
/// Representing these as an enum prevents arbitrary renderer-provided labels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LocalFileAction {
    Read,
    CreateNew,
}

impl LocalFileAction {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::CreateNew => "create_new",
        }
    }
}

/// All inputs MUST come from trusted, Rust-owned prepared state, never from
/// `studio_approve_local_file_operation`'s renderer arguments.
///
/// Read approvals carry a bounded snapshot SHA-256 which is rechecked at
/// dispatch; create approvals carry the SHA-256 of immutable prepared bytes.
pub(crate) struct NativeConsentRequest<'a> {
    pub operation_id: &'a str,
    pub task_id: &'a str,
    pub canonical_path: &'a Path,
    pub action: LocalFileAction,
    pub sha256: Option<&'a str>,
    /// Server-owned prepared UTF-8 byte length. Renderer approval payloads
    /// cannot supply or change this private consent detail.
    pub byte_count: usize,
    pub operation_digest: &'a str,
    /// Original immutable UTF-8 bytes held by the native tool host; never a
    /// replacement supplied by the approval IPC command.
    pub create_content: Option<&'a str>,
}

/// Sealed evidence of an actual native YES action for one immutable operation.
/// It has private fields, no Clone/Default/Deserialize, and no public
/// production constructor. A renderer-supplied Boolean cannot manufacture it.
#[must_use]
pub(crate) struct NativeConsentProof {
    operation_id: String,
    operation_digest: String,
}

impl NativeConsentProof {
    /// The dispatcher must check both values against its currently pending
    /// immutable operation while it holds the task/dispatch gate.
    pub(crate) fn verify(&self, operation_id: &str, operation_digest: &str) -> bool {
        self.operation_id == operation_id && self.operation_digest == operation_digest
    }

    #[cfg(test)]
    pub(crate) fn fixture_for_test(operation_id: &str, operation_digest: &str) -> Self {
        Self {
            operation_id: operation_id.into(),
            operation_digest: operation_digest.into(),
        }
    }
}

/// Public errors deliberately contain no user-supplied path, document text,
/// digest, or other material that might be copied into an application log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NativeConsentError {
    InvalidReview,
    InvalidWindow,
    DialogUnavailable,
    // Constructed only on non-Windows in production; retained for the shared
    // typed error contract and Windows-only formatter tests.
    #[cfg_attr(target_os = "windows", allow(dead_code))]
    UnsupportedPlatform,
}

impl fmt::Display for NativeConsentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let description = match self {
            Self::InvalidReview => "Native approval details are unsafe or incomplete; denied",
            Self::InvalidWindow => "A trusted native owner window is required; approval denied",
            Self::DialogUnavailable => "The native approval dialog could not open; denied",
            Self::UnsupportedPlatform => {
                "Independent native file approval is not supported on this platform; denied"
            }
        };
        f.write_str(description)
    }
}

impl Error for NativeConsentError {}

/// Prevent path content from visually substituting dialog labels, hiding
/// suffixes, or injecting extra lines/RTL reordering into a confirmation.
/// Ordinary non-ASCII names (including Persian) remain usable.
fn display_safe(value: &str) -> bool {
    !value.is_empty()
        && !value.chars().any(|character| {
            character.is_control()
                || matches!(
                    character as u32,
                    0x00ad
                        | 0x00a0
                        | 0x034f
                        | 0x061c
                        | 0x1680
                        | 0x180e
                        | 0x2000..=0x200f
                        | 0x2028..=0x202f
                        | 0x205f..=0x206f
                        | 0x115f..=0x1160
                        | 0x17b4..=0x17b5
                        | 0x3000
                        | 0x3164
                        | 0xfe00..=0xfe0f
                        | 0xfeff
                        | 0xffa0
                        | 0xe0000..=0xe0fff
                )
        })
}

fn valid_uuid(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 36
        && bytes.iter().enumerate().all(|(index, byte)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                *byte == b'-'
            } else {
                byte.is_ascii_hexdigit()
            }
        })
}

fn valid_digest(value: &str) -> bool {
    value.len() == 64 && value.as_bytes().iter().all(u8::is_ascii_hexdigit)
}

// The complete, untruncated canonical path is essential for meaningful
// consent. Fail closed if it does not fit (no ellipsis or lossy conversions).
const MAX_PATH_UTF16: usize = 4096;
const MAX_DIALOG_UTF16: usize = 6144;
const MAX_CREATE_BYTES: usize = 16 * 1024;
// MessageBoxW is a native, deliberately short first-stage consent prompt.
// Refuse longer paths instead of asking a human to approve a clipped label.
const MAX_READ_INTENT_PATH_BYTES: usize = 240;
const MAX_READ_INTENT_PROMPT_UTF16: usize = 1024;

/// Pre-inspection consent contains only a task identity and the exact lexical
/// path supplied by a model. This pure formatter must never resolve, open,
/// stat or hash that path. The later SHA-bound native approval is separate.
fn format_read_intent_prompt(
    task_id: &str,
    relative_path: &str,
) -> Result<String, NativeConsentError> {
    if !valid_uuid(task_id)
        || relative_path.is_empty()
        || relative_path.len() > MAX_READ_INTENT_PATH_BYTES
        // Model paths are ASCII at their parser boundary. For the manual-read
        // flow, allow visible Unicode filenames such as Persian, with the
        // same control/bidi-invisible exclusions as the final native dialog.
        // Refuse punctuation, slash confusables and mixed-script lookalikes
        // which might misidentify a read target at the first consent stage.
        || !display_safe(relative_path)
        || relative_path.chars().any(|ch| {
            matches!(ch, '\\' | ':' | '<' | '>' | '|' | '"' | '?' | '*')
                || matches!(
                    ch as u32,
                    0x0370..=0x052f // Greek/Cyrillic letters that resemble Latin glyphs.
                        | 0x1c80..=0x1c8f
                        | 0x2de0..=0x2dff
                        | 0xa640..=0xa69f
                        | 0xff00..=0xffef // Fullwidth punctuation and lookalike separators.
                        | 0x2044 // Fraction slash.
                        | 0x2215 // Division slash.
                        | 0x2571 // Box-drawing diagonal slash.
                        | 0x29f8 // Big solidus.
                        | 0xfe68 // Small reverse solidus.
                )
        })
        || relative_path.split('/').any(|part| {
            part.is_empty()
                || part == "."
                || part == ".."
                || part.starts_with(' ')
                || part.ends_with(' ')
                || part.ends_with('.')
                || {
                    // Windows reserved device names apply even when followed
                    // by an extension (e.g. CON.txt) and must not be sent to
                    // a privileged file reader as an apparent ordinary file.
                    let stem = part.split('.').next().unwrap_or("");
                    let upper = stem.to_ascii_uppercase();
                    matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL")
                        || (upper.len() == 4
                            && (upper.starts_with("COM") || upper.starts_with("LPT"))
                            && matches!(upper.as_bytes()[3], b'1'..=b'9'))
                }
        })
    {
        return Err(NativeConsentError::InvalidReview);
    }

    let text = format!(
        "FIRST permission request — inspect a named local file?\r\n\r\n\
         This task requests inspection of a file inside the app's private artifacts folder.\r\n\
         No file must be opened, measured, or hashed before this decision.\r\n\r\n\
         Task ID: {task_id}\r\n\
         Exact relative path: \"{}\"\r\n\r\n\
         YES permits inspection of this path for a later, separate native approval.\r\n\
         YES does NOT authorize reading contents into the app or sending them to a model.\r\n\
         A second native dialog must display the resolved path, byte count, and SHA-256.\r\n\r\n\
         NO is the default. Choose NO if you did not request this file.\r\n",
        readable_preview(relative_path)
    );
    if text.encode_utf16().count() > MAX_READ_INTENT_PROMPT_UTF16 {
        return Err(NativeConsentError::InvalidReview);
    }
    Ok(text)
}

/// Every byte appears in this text as a two-digit hex value with an offset.
/// No Unicode glyph, newline conversion, font, bidi mark, or copy operation
/// can alter the meaning of the bytes being approved.
fn exact_hex_view(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len() * 4 + 256);
    for (row, chunk) in bytes.chunks(16).enumerate() {
        let _ = write!(text, "{:08X}: ", row * 16);
        for byte in chunk {
            let _ = write!(text, "{byte:02X} ");
        }
        text.push_str("\r\n");
    }
    text
}

/// Native read-only preview. Escape controls, backslashes and Unicode format
/// marks so a misleading pasted payload cannot impersonate dialog chrome.
/// The hexadecimal byte view remains authoritative even if a glyph is
/// ambiguous in the OS font.
fn readable_preview(content: &str) -> String {
    let mut preview = String::with_capacity(content.len() + 128);
    for ch in content.chars() {
        match ch {
            '\\' => preview.push_str("\\\\"),
            '\r' => preview.push_str("\\r"),
            '\n' => preview.push_str("\\n\r\n"),
            '\t' => preview.push_str("\\t"),
            _ if !display_safe(&ch.to_string())
                || matches!(ch as u32, 0x115f..=0x1160 | 0x17b4..=0x17b5 | 0xe0000..=0xe0fff) =>
            {
                let _ = write!(preview, "\\u{{{:04X}}}", ch as u32);
            }
            _ => preview.push(ch),
        }
    }
    preview
}

struct NativeReview {
    details: String,
    preview: String,
    hex: String,
}

/// Pure, testable prompt formatter. No OS calls and no side effects.
fn format_prompt(request: &NativeConsentRequest<'_>) -> Result<String, NativeConsentError> {
    if !valid_uuid(request.operation_id)
        || !valid_uuid(request.task_id)
        || !valid_digest(request.operation_digest)
    {
        return Err(NativeConsentError::InvalidReview);
    }
    if !request.canonical_path.is_absolute() {
        return Err(NativeConsentError::InvalidReview);
    }
    // to_str refuses unpaired surrogates; to_string_lossy would silently
    // misidentify a file in the security-critical confirmation.
    let canonical = request
        .canonical_path
        .to_str()
        .ok_or(NativeConsentError::InvalidReview)?;
    if !display_safe(canonical) || canonical.encode_utf16().count() > MAX_PATH_UTF16 {
        return Err(NativeConsentError::InvalidReview);
    }

    let (hash_text, warning) = match (request.action, request.sha256, request.create_content) {
        (LocalFileAction::CreateNew, Some(digest), Some(content))
            if valid_digest(digest)
                && !content.is_empty()
                && content.len() <= MAX_CREATE_BYTES
                && request.byte_count == content.len()
                && !content.contains('\0')
                && format!("{:x}", Sha256::digest(content.as_bytes())) == digest =>
        {
            (
                digest,
                "CreateNew: review the complete source preview AND hexadecimal bytes below. The hex view is authoritative. No existing file may be replaced.",
            )
        }
        (LocalFileAction::Read, Some(digest), None)
            if valid_digest(digest) && request.byte_count <= MAX_CREATE_BYTES =>
        {
            (
                digest,
                "READ: approval is bound to the file SHA-256 shown above. The executor rejects changed bytes; the content is not displayed in this dialog.",
            )
        }
        _ => return Err(NativeConsentError::InvalidReview),
    };

    let body = format!(
        "Constellation — native local file approval\r\n\r\n\
         A file operation was requested. Deny it unless you recognize ALL details below.\r\n\r\n\
         Operation ID: {}\r\n\
         Task ID: {}\r\n\
         Action: {}\r\n\
         Canonical path (complete):\r\n{}\r\n\
         SHA-256: {}\r\n\
         Byte count: {}\r\n\
         Immutable operation digest: {}\r\n\r\n\
         {}\r\n\r\n\
         YES authorizes THIS ONE operation; NO is selected by default.\r\n\
         No/closing denies.\r\n",
        request.operation_id,
        request.task_id,
        request.action.as_str(),
        canonical,
        hash_text,
        request.byte_count,
        request.operation_digest,
        warning
    );
    if body.encode_utf16().count() > MAX_DIALOG_UTF16 {
        return Err(NativeConsentError::InvalidReview);
    }
    Ok(body)
}

fn format_review(request: &NativeConsentRequest<'_>) -> Result<NativeReview, NativeConsentError> {
    let details = format_prompt(request)?;
    let (preview, hex) = match request.create_content {
        Some(content) => (
            readable_preview(content),
            exact_hex_view(content.as_bytes()),
        ),
        None => (
            "Read operation: SHA-256 snapshot is pinned. The file content is not displayed; only matching bytes may be returned after approval.".into(),
            "No content hex preview is available for a read. The executor verifies the pinned SHA-256 before returning content.".into(),
        ),
    };
    Ok(NativeReview {
        details,
        preview,
        hex,
    })
}

/// The native YES button is the sole production constructor of a proof.
/// The HWND must come from the trusted Tauri window, never from the renderer.
/// This function must never be invoked in a unit test.
#[cfg(target_os = "windows")]
pub(crate) fn confirm_native_consent(
    owner_hwnd: isize,
    request: &NativeConsentRequest<'_>,
) -> Result<Option<NativeConsentProof>, NativeConsentError> {
    let review = format_review(request)?;
    if native_windows::show_dialog(owner_hwnd, &review)? {
        Ok(Some(NativeConsentProof {
            operation_id: request.operation_id.to_owned(),
            operation_digest: request.operation_digest.to_owned(),
        }))
    } else {
        Ok(None)
    }
}

/// FIRST of two independent Windows-native read consents. Call this before
/// preparing/opening/hashing a model-proposed read. Only a verified native
/// Tauri-owned HWND from this process may be supplied; never take it from IPC.
/// A `true` merely permits trusted Rust to inspect the lexical target and
/// construct a second, SHA-bound native confirmation. It is not a file grant,
/// cannot replace `confirm_native_consent`, and must be paired with a fresh
/// task-state/cancellation check by its trusted caller after the prompt.
#[cfg(target_os = "windows")]
pub(crate) fn confirm_native_read_intent(
    owner_hwnd: isize,
    task_id: &str,
    relative_path: &str,
) -> Result<bool, String> {
    let prompt =
        format_read_intent_prompt(task_id, relative_path).map_err(|error| error.to_string())?;
    native_windows::show_read_intent(owner_hwnd, &prompt).map_err(|error| error.to_string())
}

/// No browser, terminal, or unsupported-platform fallback may approve a read.
#[cfg(not(target_os = "windows"))]
pub(crate) fn confirm_native_read_intent(
    _owner_hwnd: isize,
    _task_id: &str,
    _relative_path: &str,
) -> Result<bool, String> {
    Err(NativeConsentError::UnsupportedPlatform.to_string())
}

/// Private Win32 window. Two read-only native multiline EDIT controls provide
/// a readable view and an exact, complete scrollable byte view. MessageBoxW
/// cannot promise scrolling or complete visibility for a 16KiB document.
#[cfg(target_os = "windows")]
mod native_windows {
    use super::{NativeConsentError, NativeReview};
    use std::{ffi::c_void, mem, ptr};

    type Hwnd = *mut c_void;
    type Hinstance = *mut c_void;
    type Hmenu = *mut c_void;
    const CLASS_NAME: &str = "ConstellationNativeConsentV2";
    const WM_NCCREATE: u32 = 0x0081;
    const WM_NCDESTROY: u32 = 0x0082;
    const WM_COMMAND: u32 = 0x0111;
    const WM_CLOSE: u32 = 0x0010;
    const WM_DESTROY: u32 = 0x0002;
    const WM_SETFONT: u32 = 0x0030;
    const EM_LIMITTEXT: u32 = 0x00c5;
    const GWLP_USERDATA: i32 = -21;
    const WS_VISIBLE: u32 = 0x10000000;
    const WS_CHILD: u32 = 0x40000000;
    const WS_BORDER: u32 = 0x00800000;
    const WS_VSCROLL: u32 = 0x00200000;
    const WS_HSCROLL: u32 = 0x00100000;
    const WS_TABSTOP: u32 = 0x00010000;
    const WS_SYSMENU: u32 = 0x00080000;
    const WS_CAPTION: u32 = 0x00c00000;
    const ES_MULTILINE: u32 = 0x0004;
    const ES_AUTOVSCROLL: u32 = 0x0040;
    const ES_AUTOHSCROLL: u32 = 0x0080;
    const ES_READONLY: u32 = 0x0800;
    const BS_DEFPUSHBUTTON: u32 = 0x0001;
    const WS_EX_DLGMODALFRAME: u32 = 0x00000001;
    const WS_EX_CONTROLPARENT: u32 = 0x00010000;
    const IDYES: usize = 6;
    const IDNO: usize = 7;
    const IDCANCEL: usize = 2;
    const DEFAULT_GUI_FONT: i32 = 17;
    const ERROR_CLASS_ALREADY_EXISTS: u32 = 1410;

    #[repr(C)]
    struct WndClassW {
        style: u32,
        wnd_proc: Option<unsafe extern "system" fn(Hwnd, u32, usize, isize) -> isize>,
        cls_extra: i32,
        wnd_extra: i32,
        instance: Hinstance,
        icon: Hwnd,
        cursor: Hwnd,
        background: Hwnd,
        menu_name: *const u16,
        class_name: *const u16,
    }

    #[repr(C)]
    struct CreateStructW {
        create_params: *mut c_void,
        instance: Hinstance,
        menu: Hmenu,
        parent: Hwnd,
        cy: i32,
        cx: i32,
        y: i32,
        x: i32,
        style: i32,
        name: *const u16,
        class_name: *const u16,
        ex_style: u32,
    }

    #[repr(C)]
    struct Point {
        x: i32,
        y: i32,
    }

    #[repr(C)]
    struct Message {
        hwnd: Hwnd,
        message: u32,
        wparam: usize,
        lparam: isize,
        time: u32,
        pt: Point,
        private: u32,
    }

    #[repr(C)]
    struct Rect {
        left: i32,
        top: i32,
        right: i32,
        bottom: i32,
    }

    #[link(name = "user32")]
    unsafe extern "system" {
        fn MessageBoxW(owner: Hwnd, text: *const u16, caption: *const u16, flags: u32) -> i32;
        fn RegisterClassW(class: *const WndClassW) -> u16;
        fn CreateWindowExW(
            ex_style: u32,
            class: *const u16,
            title: *const u16,
            style: u32,
            x: i32,
            y: i32,
            width: i32,
            height: i32,
            parent: Hwnd,
            menu: Hmenu,
            instance: Hinstance,
            param: *mut c_void,
        ) -> Hwnd;
        fn DestroyWindow(hwnd: Hwnd) -> i32;
        fn DefWindowProcW(hwnd: Hwnd, msg: u32, wp: usize, lp: isize) -> isize;
        fn SetWindowLongPtrW(hwnd: Hwnd, index: i32, value: isize) -> isize;
        fn GetWindowLongPtrW(hwnd: Hwnd, index: i32) -> isize;
        fn IsWindow(hwnd: Hwnd) -> i32;
        fn IsWindowVisible(hwnd: Hwnd) -> i32;
        fn IsWindowEnabled(hwnd: Hwnd) -> i32;
        fn EnableWindow(hwnd: Hwnd, enable: i32) -> i32;
        fn GetWindowThreadProcessId(hwnd: Hwnd, process_id: *mut u32) -> u32;
        fn GetThreadDesktop(thread_id: u32) -> Hwnd;
        fn SetThreadDesktop(desktop: Hwnd) -> i32;
        fn GetAncestor(hwnd: Hwnd, flag: u32) -> Hwnd;
        fn ShowWindow(hwnd: Hwnd, command: i32) -> i32;
        fn UpdateWindow(hwnd: Hwnd) -> i32;
        fn SetForegroundWindow(hwnd: Hwnd) -> i32;
        fn SetFocus(hwnd: Hwnd) -> Hwnd;
        fn GetClientRect(hwnd: Hwnd, rect: *mut Rect) -> i32;
        fn GetSystemMetrics(index: i32) -> i32;
        fn GetMessageW(message: *mut Message, hwnd: Hwnd, min: u32, max: u32) -> i32;
        fn TranslateMessage(message: *const Message) -> i32;
        fn DispatchMessageW(message: *const Message) -> isize;
        fn IsDialogMessageW(hwnd: Hwnd, message: *mut Message) -> i32;
        fn SendMessageW(hwnd: Hwnd, message: u32, wp: usize, lp: isize) -> isize;
        fn SetWindowTextW(hwnd: Hwnd, text: *const u16) -> i32;
        fn GetWindowTextLengthW(hwnd: Hwnd) -> i32;
        fn LoadCursorW(instance: Hinstance, cursor: *const u16) -> Hwnd;
        fn PostQuitMessage(exit_code: i32);
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetModuleHandleW(name: *const u16) -> Hinstance;
        fn GetCurrentProcessId() -> u32;
        fn GetCurrentThreadId() -> u32;
        fn GetLastError() -> u32;
    }
    #[link(name = "gdi32")]
    unsafe extern "system" {
        fn GetStockObject(index: i32) -> Hwnd;
    }

    #[derive(Default)]
    struct DialogState {
        accepted: bool,
        closed: bool,
        // Only a BN_CLICKED notification from this dialog's YES button may
        // authorize an operation. Bare WM_COMMAND messages are not consent.
        yes_button: isize,
    }

    fn yes_button_clicked(wparam: usize, lparam: isize, expected_button: isize) -> bool {
        // HIWORD(wParam) is BN_CLICKED (zero); LOWORD is the control ID.
        // lParam identifies the native BUTTON that emitted the notification.
        wparam == IDYES && expected_button != 0 && lparam == expected_button
    }

    unsafe extern "system" fn wndproc(
        hwnd: Hwnd,
        message: u32,
        wparam: usize,
        lparam: isize,
    ) -> isize {
        if message == WM_NCCREATE {
            // SAFETY: the CREATESTRUCT and its stable stack-backed state are
            // owned by CreateWindowExW and remain live until DestroyWindow.
            let create = unsafe { &*(lparam as *const CreateStructW) };
            if create.create_params.is_null() {
                return 0;
            }
            unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.create_params as isize) };
        }
        let state = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut DialogState };
        match message {
            WM_COMMAND => {
                let button = wparam & 0xffff;
                if button == IDYES {
                    if !state.is_null()
                        && yes_button_clicked(wparam, lparam, unsafe { (*state).yes_button })
                    {
                        unsafe { (*state).accepted = true };
                        unsafe { DestroyWindow(hwnd) };
                    }
                    return 0;
                }
                if button == IDNO || button == IDCANCEL {
                    if !state.is_null() {
                        // Denial remains the default for Escape and close.
                        unsafe { (*state).accepted = false };
                    }
                    unsafe { DestroyWindow(hwnd) };
                    return 0;
                }
            }
            WM_CLOSE => {
                unsafe { DestroyWindow(hwnd) };
                return 0;
            }
            WM_DESTROY => {
                if !state.is_null() {
                    unsafe { (*state).closed = true };
                }
                return 0;
            }
            WM_NCDESTROY => {
                unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0) };
            }
            _ => {}
        }
        unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
    }

    fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// Genuine parent-owned Win32 dialog before any private filesystem access.
    /// MessageBoxW performs its own modal window management; IDYES is its
    /// only successful response. Both ESC/close and NO remain denial.
    pub(super) fn show_read_intent(
        owner_hwnd: isize,
        prompt: &str,
    ) -> Result<bool, NativeConsentError> {
        const MB_YESNO: u32 = 0x0004;
        const MB_ICONWARNING: u32 = 0x0030;
        const MB_DEFBUTTON2: u32 = 0x0100;
        let owner = owner_hwnd as Hwnd;
        let mut pid = 0;
        let owner_thread = if owner.is_null() {
            0
        } else {
            unsafe { GetWindowThreadProcessId(owner, &mut pid) }
        };
        if owner.is_null()
            || unsafe { IsWindow(owner) } == 0
            || unsafe { IsWindowVisible(owner) } == 0
            || unsafe { IsWindowEnabled(owner) } == 0
            || owner_thread == 0
            || pid != unsafe { GetCurrentProcessId() }
            || unsafe { GetAncestor(owner, 2) } != owner
        {
            return Err(NativeConsentError::InvalidWindow);
        }
        // Tauri commands may run on threads assigned a different Win32
        // desktop. Never mistake an invisible consent box for human approval.
        let owner_desktop = unsafe { GetThreadDesktop(owner_thread) };
        let current_desktop = unsafe { GetThreadDesktop(GetCurrentThreadId()) };
        if owner_desktop.is_null() || current_desktop.is_null() {
            return Err(NativeConsentError::DialogUnavailable);
        }
        if owner_desktop != current_desktop && unsafe { SetThreadDesktop(owner_desktop) } == 0 {
            return Err(NativeConsentError::DialogUnavailable);
        }
        let caption = wide("Constellation — FIRST permission to inspect file");
        let text = wide(prompt);
        match unsafe {
            MessageBoxW(
                owner,
                text.as_ptr(),
                caption.as_ptr(),
                MB_YESNO | MB_ICONWARNING | MB_DEFBUTTON2,
            )
        } {
            6 => Ok(true),      // Win32 IDYES.
            7 | 2 => Ok(false), // IDNO / IDCANCEL (including window close).
            _ => Err(NativeConsentError::DialogUnavailable),
        }
    }

    struct OwnerGuard {
        hwnd: Hwnd,
        thread_id: u32,
        process_id: u32,
    }
    impl Drop for OwnerGuard {
        fn drop(&mut self) {
            // Restore the owner on every failure path, unless the HWND has
            // been destroyed/reused by a different owner thread or process.
            let mut pid = 0;
            if unsafe { IsWindow(self.hwnd) } != 0
                && unsafe { GetWindowThreadProcessId(self.hwnd, &mut pid) } == self.thread_id
                && pid == self.process_id
            {
                unsafe { EnableWindow(self.hwnd, 1) };
            }
        }
    }

    /// All controls are native read-only EDIT, STATIC and BUTTON windows.
    /// Text is set in full and the resulting UTF-16 length checked before
    /// any approval button is displayed.
    unsafe fn child(
        parent: Hwnd,
        class: &str,
        text: &str,
        style: u32,
        id: usize,
        bounds: (i32, i32, i32, i32),
        handles: (Hinstance, Hwnd),
    ) -> Result<Hwnd, NativeConsentError> {
        let (instance, font) = handles;
        let (x, y, width, height) = bounds;
        let class = wide(class);
        let hwnd = unsafe {
            CreateWindowExW(
                0,
                class.as_ptr(),
                ptr::null(),
                WS_CHILD | WS_VISIBLE | style,
                x,
                y,
                width,
                height,
                parent,
                id as Hmenu,
                instance,
                ptr::null_mut(),
            )
        };
        if hwnd.is_null() {
            return Err(NativeConsentError::DialogUnavailable);
        }
        let chars = wide(text);
        unsafe {
            // The default EDIT input limit must not truncate the exact hex
            // view; EM_LIMITTEXT does not apply to SetWindowTextW but raising
            // it prevents surprising native edit defaults.
            SendMessageW(hwnd, EM_LIMITTEXT, chars.len() + 1, 0);
            SendMessageW(hwnd, WM_SETFONT, font as usize, 1);
            if SetWindowTextW(hwnd, chars.as_ptr()) == 0
                || GetWindowTextLengthW(hwnd) != (chars.len() - 1) as i32
            {
                return Err(NativeConsentError::DialogUnavailable);
            }
        }
        Ok(hwnd)
    }

    pub(super) fn show_dialog(
        owner_hwnd: isize,
        review: &NativeReview,
    ) -> Result<bool, NativeConsentError> {
        let owner = owner_hwnd as Hwnd;
        let mut pid = 0;
        let owner_thread = if owner.is_null() {
            0
        } else {
            unsafe { GetWindowThreadProcessId(owner, &mut pid) }
        };
        // Prevent a forged, stale, foreign-process or child HWND from becoming
        // the modal owner; reject a window already in another modal sequence.
        if owner.is_null()
            || unsafe { IsWindow(owner) } == 0
            || owner_thread == 0
            || pid != unsafe { GetCurrentProcessId() }
            || unsafe { GetAncestor(owner, 2) } != owner
            || unsafe { IsWindowEnabled(owner) } == 0
        {
            return Err(NativeConsentError::InvalidWindow);
        }
        // Tauri can dispatch commands on worker threads that have a different
        // Win32 desktop from the real window owner. A top-level window created
        // there cannot appear in the owner's modal UI and must never be used
        // to satisfy human consent. Bind this thread to the *verified* owner
        // thread's desktop before registering or creating any native windows.
        // SetThreadDesktop fails closed if this thread already owns windows or
        // hooks that would make switching unsafe.
        let owner_desktop = unsafe { GetThreadDesktop(owner_thread) };
        let current_desktop = unsafe { GetThreadDesktop(GetCurrentThreadId()) };
        if owner_desktop.is_null() || current_desktop.is_null() {
            return Err(NativeConsentError::DialogUnavailable);
        }
        if owner_desktop != current_desktop && unsafe { SetThreadDesktop(owner_desktop) } == 0 {
            return Err(NativeConsentError::DialogUnavailable);
        }
        let width = unsafe { GetSystemMetrics(0) };
        let height = unsafe { GetSystemMetrics(1) };
        if width < 700 || height < 650 {
            // A tiny desktop cannot show adequate review panes plus Yes/No.
            return Err(NativeConsentError::DialogUnavailable);
        }
        let w = (width - 70).min(1100);
        let h = (height - 80).min(880);
        let instance = unsafe { GetModuleHandleW(ptr::null()) };
        if instance.is_null() {
            return Err(NativeConsentError::DialogUnavailable);
        }
        let name = wide(CLASS_NAME);
        let class = WndClassW {
            style: 0,
            wnd_proc: Some(wndproc),
            cls_extra: 0,
            wnd_extra: 0,
            instance,
            icon: ptr::null_mut(),
            cursor: unsafe { LoadCursorW(ptr::null_mut(), 32512usize as *const u16) },
            background: 16usize as Hwnd, // COLOR_BTNFACE + 1
            menu_name: ptr::null(),
            class_name: name.as_ptr(),
        };
        if unsafe { RegisterClassW(&class) } == 0
            && unsafe { GetLastError() } != ERROR_CLASS_ALREADY_EXISTS
        {
            return Err(NativeConsentError::DialogUnavailable);
        }

        // Every outstanding approval reserves its own pending operation.
        // A disabled owner also prevents simultaneous competing dialogs.
        // IMPORTANT: EnableWindow returns the *previous* enabled state:
        // zero means this previously-enabled owner was successfully disabled,
        // NOT an error. The owner is checked enabled above, so always install
        // the restoration guard immediately after changing its state.
        unsafe { EnableWindow(owner, 0) };
        let _owner_guard = OwnerGuard {
            hwnd: owner,
            thread_id: owner_thread,
            process_id: pid,
        };
        // If a platform/window hook somehow keeps the owner enabled, refuse
        // to display a non-modal approval window; the guard restores on exit.
        if unsafe { IsWindowEnabled(owner) } != 0 {
            return Err(NativeConsentError::InvalidWindow);
        }
        let mut state = DialogState::default();
        let title = wide("Constellation — native file consent (review all bytes)");
        let dialog = unsafe {
            CreateWindowExW(
                WS_EX_DLGMODALFRAME | WS_EX_CONTROLPARENT,
                name.as_ptr(),
                title.as_ptr(),
                WS_CAPTION | WS_SYSMENU,
                (width - w) / 2,
                (height - h) / 2,
                w,
                h,
                owner,
                ptr::null_mut(),
                instance,
                (&raw mut state).cast::<c_void>(),
            )
        };
        if dialog.is_null() {
            return Err(NativeConsentError::DialogUnavailable);
        }
        let result = (|| -> Result<bool, NativeConsentError> {
            let mut rect = Rect {
                left: 0,
                top: 0,
                right: 0,
                bottom: 0,
            };
            if unsafe { GetClientRect(dialog, &mut rect) } == 0 {
                return Err(NativeConsentError::DialogUnavailable);
            }
            let cw = rect.right - rect.left;
            let ch = rect.bottom - rect.top;
            let pad = 16;
            let usable = cw - pad * 2;
            let font = unsafe { GetStockObject(DEFAULT_GUI_FONT) };
            let edit_style = WS_BORDER
                | WS_VSCROLL
                | WS_HSCROLL
                | WS_TABSTOP
                | ES_MULTILINE
                | ES_AUTOVSCROLL
                | ES_AUTOHSCROLL
                | ES_READONLY;
            let label = |text: &str, y: i32| unsafe {
                child(
                    dialog,
                    "STATIC",
                    text,
                    0,
                    0,
                    (pad, y, usable, 32),
                    (instance, font),
                )
            };
            label(
                "Native consent. Read every field; NO is the default. Closing denies.",
                10,
            )?;
            unsafe {
                child(
                    dialog,
                    "EDIT",
                    &review.details,
                    edit_style,
                    10,
                    (pad, 45, usable, 152),
                    (instance, font),
                )?;
            }
            label(
                "Readable content preview: special characters are escaped (CreateNew only)",
                205,
            )?;
            let preview_h = ((ch - 400) / 2).max(104);
            unsafe {
                child(
                    dialog,
                    "EDIT",
                    &review.preview,
                    edit_style,
                    11,
                    (pad, 239, usable, preview_h),
                    (instance, font),
                )?;
            }
            let hex_label_y = 246 + preview_h;
            label(
                "Exact UTF-8 bytes, HEX with 8-digit offsets. Review all rows before YES.",
                hex_label_y,
            )?;
            let hex_y = hex_label_y + 34;
            let buttons_y = ch - 52;
            let hex_h = buttons_y - hex_y - 14;
            if hex_h < 85 {
                return Err(NativeConsentError::DialogUnavailable);
            }
            unsafe {
                child(
                    dialog,
                    "EDIT",
                    &review.hex,
                    edit_style,
                    12,
                    (pad, hex_y, usable, hex_h),
                    (instance, font),
                )?;
            }
            let yes = unsafe {
                child(
                    dialog,
                    "BUTTON",
                    "YES — approve this one operation",
                    WS_TABSTOP,
                    IDYES,
                    (cw - 402, buttons_y, 240, 34),
                    (instance, font),
                )?
            };
            state.yes_button = yes as isize;
            let no = unsafe {
                child(
                    dialog,
                    "BUTTON",
                    "NO — deny (default)",
                    WS_TABSTOP | BS_DEFPUSHBUTTON,
                    IDNO,
                    (cw - 151, buttons_y, 135, 34),
                    (instance, font),
                )?
            };
            unsafe {
                ShowWindow(dialog, 5);
                UpdateWindow(dialog);
                SetForegroundWindow(dialog);
                SetFocus(no);
            }
            loop {
                if state.closed {
                    break;
                }
                let mut msg: Message = unsafe { mem::zeroed() };
                let outcome = unsafe { GetMessageW(&mut msg, ptr::null_mut(), 0, 0) };
                if outcome <= 0 {
                    // Preserve a WM_QUIT intended for a host message loop.
                    // Returning an error always denies the file operation.
                    if outcome == 0 {
                        unsafe { PostQuitMessage(msg.wparam as i32) };
                    }
                    return Err(NativeConsentError::DialogUnavailable);
                }
                if unsafe { IsDialogMessageW(dialog, &mut msg) } == 0 {
                    unsafe {
                        TranslateMessage(&msg);
                        DispatchMessageW(&msg);
                    }
                }
            }
            Ok(state.accepted)
        })();
        if unsafe { IsWindow(dialog) } != 0 {
            unsafe { DestroyWindow(dialog) };
        }
        result
    }

    #[cfg(test)]
    mod tests {
        use super::{IDNO, IDYES, yes_button_clicked};

        #[test]
        fn approval_requires_actual_yes_button_notification() {
            let yes_button = 0x1234_isize;
            assert!(yes_button_clicked(IDYES, yes_button, yes_button));
            assert!(!yes_button_clicked(IDYES, 0, yes_button));
            assert!(!yes_button_clicked(IDYES, yes_button, 0));
            assert!(!yes_button_clicked(IDYES, 0x5678, yes_button));
            assert!(!yes_button_clicked(IDNO, yes_button, yes_button));
            assert!(!yes_button_clicked(
                IDYES | (1 << 16),
                yes_button,
                yes_button
            ));
        }
    }
}

/// Never silently fall back to the renderer's own approval on an unsupported
/// OS. A new independently reviewed platform-specific implementation is
/// required before file effects can be granted there.
#[cfg(not(target_os = "windows"))]
pub(crate) fn confirm_native_consent(
    _owner_hwnd: isize,
    _request: &NativeConsentRequest<'_>,
) -> Result<Option<NativeConsentProof>, NativeConsentError> {
    Err(NativeConsentError::UnsupportedPlatform)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    const TASK: &str = "11111111-2222-4333-8444-555555555555";
    const OP: &str = "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee";
    const CONTENT: &str = "hello\nSecond line";

    fn sample_path() -> PathBuf {
        std::env::temp_dir().join("Constellation").join("نمونه.txt")
    }

    fn create_request<'a>(path: &'a Path) -> NativeConsentRequest<'a> {
        NativeConsentRequest {
            operation_id: OP,
            task_id: TASK,
            canonical_path: path,
            action: LocalFileAction::CreateNew,
            sha256: Some("6785f6a0f518713df49132febed8f9fc2613ec0a20df8361df950c1595a69f4b"),
            byte_count: CONTENT.len(),
            operation_digest: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            create_content: Some(CONTENT),
        }
    }

    #[test]
    fn first_read_consent_names_task_exact_relative_path_and_second_consent() {
        let first = format_read_intent_prompt(TASK, "notes/selected-report.md").unwrap();
        assert!(first.contains(TASK));
        assert!(first.contains("Exact relative path: \"notes/selected-report.md\""));
        assert!(first.contains("FIRST permission request"));
        assert!(first.contains("No file must be opened, measured, or hashed"));
        assert!(first.contains("second native dialog"));
        assert!(first.contains("NO is the default"));
        assert!(!first.contains("SHA-256: "));
        assert!(!first.contains("Canonical path:"));
        assert!(!first.contains("Immutable operation digest:"));
        assert!(first.encode_utf16().count() <= MAX_READ_INTENT_PROMPT_UTF16);
    }

    #[test]
    fn first_read_consent_rejects_path_spoofing_traversal_and_oversized_inputs() {
        for path in [
            "",
            "/absolute.txt",
            "../escape.txt",
            "safe/../escape.txt",
            "safe/./file.txt",
            "safe//file.txt",
            "safe/",
            "safe\\file.txt",
            "C:/secret.txt",
            "\\\\server\\share\\file.txt",
            "safe:alternate-stream",
            "safe/file?.txt",
            "safe/ file.txt",
            "safe/file .txt ",
            "safe/file..",
            "CON",
            "CON.txt",
            "notes/prn.md",
            "AUX.json",
            "COM1.dat",
            "lpt9.txt",
            "safe/\"Action: YES\"",
            "safe\nAction: YES",
            "safe\r\nDefault button: YES",
            "safe\0secret",
            "safe\tname",
            "safe\u{202e}txt.exe",
            "safe\u{2066}name\u{2069}.txt",
            "safe\u{200b}name.txt",
            "safe\u{034f}name.txt",
            "safe/аbc.txt", // Cyrillic a can impersonate Latin a.
        ] {
            assert_eq!(
                format_read_intent_prompt(TASK, path),
                Err(NativeConsentError::InvalidReview),
                "unsafe read intent accepted: {path:?}"
            );
        }
        for safe_unicode in ["notes/گزارش.txt", "資料/測定.md"] {
            let prompt = format_read_intent_prompt(TASK, safe_unicode).unwrap();
            assert!(prompt.contains(safe_unicode));
        }
        let boundary = format!("notes/{}", "a".repeat(MAX_READ_INTENT_PATH_BYTES - 6));
        assert_eq!(boundary.len(), MAX_READ_INTENT_PATH_BYTES);
        assert!(format_read_intent_prompt(TASK, &boundary).is_ok());
        assert_eq!(
            format_read_intent_prompt(TASK, &format!("{boundary}x")),
            Err(NativeConsentError::InvalidReview)
        );
        for malformed_id in [
            "",
            "fake-task",
            "not-a-task\r\nAction: YES",
            OP.trim_end_matches('e'),
        ] {
            assert_eq!(
                format_read_intent_prompt(malformed_id, "safe/readme.md"),
                Err(NativeConsentError::InvalidReview)
            );
        }
    }

    #[test]
    fn first_read_consent_invalid_input_errors_never_echo_untrusted_paths() {
        let secret_path = "../private/passwords.txt";
        let failure = format_read_intent_prompt(TASK, secret_path)
            .unwrap_err()
            .to_string();
        assert!(!failure.contains("private"));
        assert!(!failure.contains(TASK));
        assert!(!failure.contains(secret_path));
    }

    #[test]
    fn exact_create_prompt_includes_all_native_fields_without_truncation() {
        let path = sample_path();
        let request = create_request(&path);
        let text = format_prompt(&request).unwrap();
        assert!(text.contains(TASK));
        assert!(text.contains("Action: create_new"));
        assert!(text.contains(path.to_str().unwrap()));
        assert!(
            text.contains(
                "SHA-256: 6785f6a0f518713df49132febed8f9fc2613ec0a20df8361df950c1595a69f4b"
            )
        );
        assert!(text.contains("Immutable operation digest: bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"));
        assert!(text.contains("NO is selected by default."));
        assert!(!text.contains("\u{fffd}"));
        assert!(text.contains(OP));
    }

    #[test]
    fn read_approval_is_bound_to_snapshot_sha256() {
        let path = sample_path();
        let mut request = create_request(&path);
        request.action = LocalFileAction::Read;
        request.create_content = None;
        let text = format_prompt(&request).unwrap();
        assert!(text.contains("Action: read"));
        assert!(text.contains(&format!("Byte count: {}", CONTENT.len())));
        assert!(
            text.contains(
                "SHA-256: 6785f6a0f518713df49132febed8f9fc2613ec0a20df8361df950c1595a69f4b"
            )
        );
        assert!(text.contains("executor rejects changed bytes"));
        let review = format_review(&request).unwrap();
        assert!(review.preview.contains("only matching bytes"));
        assert!(!review.preview.contains("Path-only"));
    }

    #[test]
    fn fail_closed_on_missing_or_malformed_read_hash() {
        let path = sample_path();
        let mut request = create_request(&path);
        request.sha256 = None;
        assert_eq!(
            format_prompt(&request),
            Err(NativeConsentError::InvalidReview)
        );
        request.action = LocalFileAction::Read;
        request.create_content = None;
        assert_eq!(
            format_prompt(&request),
            Err(NativeConsentError::InvalidReview)
        );
        request.sha256 = Some("invalid");
        assert_eq!(
            format_prompt(&request),
            Err(NativeConsentError::InvalidReview)
        );
    }

    #[test]
    fn native_read_requires_bounded_private_byte_count_and_create_requires_exact_length() {
        let path = sample_path();
        let mut request = create_request(&path);
        request.byte_count = CONTENT.len() + 1;
        assert_eq!(
            format_prompt(&request),
            Err(NativeConsentError::InvalidReview)
        );
        request.action = LocalFileAction::Read;
        request.create_content = None;
        request.byte_count = MAX_CREATE_BYTES + 1;
        assert_eq!(
            format_prompt(&request),
            Err(NativeConsentError::InvalidReview)
        );
        request.byte_count = CONTENT.len();
        let details = format_review(&request).unwrap();
        assert!(
            details
                .details
                .contains(&format!("Byte count: {}", CONTENT.len()))
        );
        assert!(
            details.details.contains(
                "SHA-256: 6785f6a0f518713df49132febed8f9fc2613ec0a20df8361df950c1595a69f4b"
            )
        );
        assert!(!details.preview.contains(CONTENT));
    }

    #[test]
    fn reject_untrusted_task_and_operation_identifiers() {
        let path = sample_path();
        let mut request = create_request(&path);
        request.task_id = "task\nAction: bypass";
        assert_eq!(
            format_prompt(&request),
            Err(NativeConsentError::InvalidReview)
        );
        request.task_id = TASK;
        request.operation_id = "invalid";
        assert_eq!(
            format_prompt(&request),
            Err(NativeConsentError::InvalidReview)
        );
        request.operation_id = OP;
        request.operation_digest = "not a SHA256 commitment";
        assert_eq!(
            format_prompt(&request),
            Err(NativeConsentError::InvalidReview)
        );
    }

    #[test]
    fn reject_injected_control_bidi_or_invisible_path_content() {
        for name in [
            "good\nAction: create_new.txt",
            "good\r\nSHA-256: fake.txt",
            "good\0hidden.txt",
            "good\u{202e}txt.exe",
            "good\u{2066}hidden\u{2069}.txt",
            "good\u{200b}hidden.txt",
            "good\u{2028}Action: spoof.txt",
            "good\u{115f}hidden.txt",
            "good\u{3164}hidden.txt",
            "good\u{ffa0}hidden.txt",
            "good\u{e0100}hidden.txt",
        ] {
            let path = std::env::temp_dir().join(name);
            let request = create_request(&path);
            assert_eq!(
                format_prompt(&request),
                Err(NativeConsentError::InvalidReview),
                "unsafe path unexpectedly accepted: {name:?}"
            );
        }
    }

    #[test]
    fn reject_relative_or_oversized_path_and_hash() {
        let relative = Path::new("notes.txt");
        assert_eq!(
            format_prompt(&create_request(relative)),
            Err(NativeConsentError::InvalidReview)
        );
        let long = std::env::temp_dir().join("x".repeat(MAX_PATH_UTF16 + 1));
        assert_eq!(
            format_prompt(&create_request(&long)),
            Err(NativeConsentError::InvalidReview)
        );
        let path = sample_path();
        let mut request = create_request(&path);
        request.sha256 = Some("abcdef");
        assert_eq!(
            format_prompt(&request),
            Err(NativeConsentError::InvalidReview)
        );
    }

    #[test]
    fn errors_never_include_a_sensitive_path_or_digest() {
        for error in [
            NativeConsentError::InvalidReview,
            NativeConsentError::InvalidWindow,
            NativeConsentError::DialogUnavailable,
            NativeConsentError::UnsupportedPlatform,
        ] {
            let message = error.to_string();
            assert!(!message.contains("secret"));
            assert!(!message.contains(TASK));
            assert!(!message.is_empty());
        }
    }

    #[test]
    fn exact_hex_view_contains_all_bytes_and_offsets_for_full_sized_input() {
        let bytes: Vec<u8> = (0..MAX_CREATE_BYTES).map(|index| index as u8).collect();
        let view = exact_hex_view(&bytes);
        assert_eq!(view.lines().count(), MAX_CREATE_BYTES / 16);
        assert!(view.starts_with("00000000: 00 01 02 03"));
        assert!(view.contains("00003FF0: F0 F1 F2 F3"));
        let recovered = view
            .lines()
            .flat_map(|line| {
                line.split_once(": ")
                    .unwrap()
                    .1
                    .split_whitespace()
                    .map(|pair| u8::from_str_radix(pair, 16).unwrap())
            })
            .collect::<Vec<_>>();
        assert_eq!(recovered, bytes);
    }

    #[test]
    fn create_review_accepts_exact_byte_limit_and_rejects_one_byte_more() {
        let path = sample_path();
        let content = "x".repeat(MAX_CREATE_BYTES);
        let digest = format!("{:x}", Sha256::digest(content.as_bytes()));
        let mut request = create_request(&path);
        request.create_content = Some(&content);
        request.byte_count = content.len();
        request.sha256 = Some(&digest);
        request.byte_count = content.len();
        let review = format_review(&request).unwrap();
        assert_eq!(review.preview.len(), MAX_CREATE_BYTES);
        assert_eq!(review.hex.lines().count(), MAX_CREATE_BYTES / 16);
        assert!(review.hex.contains("00003FF0: 78 78 78"));

        let oversize = "x".repeat(MAX_CREATE_BYTES + 1);
        let oversize_digest = format!("{:x}", Sha256::digest(oversize.as_bytes()));
        request.create_content = Some(&oversize);
        request.byte_count = oversize.len();
        request.sha256 = Some(&oversize_digest);
        request.byte_count = oversize.len();
        assert!(matches!(
            format_review(&request),
            Err(NativeConsentError::InvalidReview)
        ));
    }

    #[test]
    fn readable_preview_escapes_tricks_without_losing_hex_bytes() {
        let input = "Persian: سلام\r\nX\\Y\t\u{202e}right\u{200b}zero\u{0007}alert";
        let view = readable_preview(input);
        assert!(view.contains("سلام"));
        assert!(view.contains("\\r\\n\r\n"));
        assert!(view.contains("X\\\\Y\\t"));
        assert!(view.contains("\\u{202E}"));
        assert!(view.contains("\\u{200B}"));
        assert!(view.contains("\\u{0007}"));
        assert!(!view.contains('\u{202e}'));
        assert!(!view.contains('\u{200b}'));
        let hex = exact_hex_view(input.as_bytes());
        let decoded = hex
            .lines()
            .flat_map(|line| {
                line.split_once(": ")
                    .unwrap()
                    .1
                    .split_whitespace()
                    .map(|byte| u8::from_str_radix(byte, 16).unwrap())
            })
            .collect::<Vec<_>>();
        assert_eq!(decoded, input.as_bytes());
    }

    #[test]
    fn reject_modified_or_missing_native_content_even_if_hash_looks_valid() {
        let path = sample_path();
        let mut request = create_request(&path);
        request.create_content = Some("unprepared content");
        assert_eq!(
            format_prompt(&request),
            Err(NativeConsentError::InvalidReview)
        );
        request.create_content = None;
        assert_eq!(
            format_prompt(&request),
            Err(NativeConsentError::InvalidReview)
        );
        request.create_content = Some("");
        assert_eq!(
            format_prompt(&request),
            Err(NativeConsentError::InvalidReview)
        );
        request.create_content = Some("x\0y");
        assert_eq!(
            format_prompt(&request),
            Err(NativeConsentError::InvalidReview)
        );
    }

    #[test]
    fn proof_only_matches_exact_operation_and_digest() {
        let proof = NativeConsentProof::fixture_for_test(OP, "a".repeat(64).as_str());
        assert!(proof.verify(OP, "a".repeat(64).as_str()));
        assert!(!proof.verify(TASK, "a".repeat(64).as_str()));
        assert!(!proof.verify(OP, "b".repeat(64).as_str()));
    }
}
