# OS-level two-stage read intent + ConstellationNativeConsentV2 acceptance for an isolated Constellation QA
# process. The caller MUST verify the tested EXE hash, its HWND, and that the
# worker is attached to a private non-input Win32 desktop first.
# No app is launched here; no keyboard, mouse, focus, clipboard or SwitchDesktop.

if (-not ('ConstellationPrivateConsentDialog' -as [type])) {
  $interop = @'
using System;
using System.Runtime.InteropServices;
using System.Text;
using System.Threading.Tasks;
using System.Windows.Automation;
using System.Globalization;
using System.Collections.Generic;

public static class ConstellationPrivateConsentDialog {
  public const string Title = "Constellation — native file consent (review all bytes)";
  public const string ClassName = "ConstellationNativeConsentV2";
  public const string ReadIntentTitle = "Constellation — FIRST permission to inspect file";
  public const string ReadIntentClass = "#32770";
  public const int YesId = 6, NoId = 7;
  private const uint DM_GETDEFID = 0x0400, DC_HASDEFID = 0x534B;
  private const int GW_OWNER = 4;
  private const int GWL_STYLE = -16;
  private const long BS_TYPEMASK = 0x0f, BS_DEFPUSHBUTTON = 0x01;
  private const long ES_READONLY = 0x0800, ES_MULTILINE = 0x0004;
  private const long WS_VSCROLL = 0x00200000, WS_HSCROLL = 0x00100000;
  private delegate bool EnumWindowsCallback(IntPtr hwnd, IntPtr argument);
  [StructLayout(LayoutKind.Sequential)]
  private struct GuiThreadInfo {
    public uint cbSize, flags;
    public IntPtr hwndActive, hwndFocus, hwndCapture, hwndMenuOwner;
    public IntPtr hwndMoveSize, hwndCaret;
    public int left, top, right, bottom;
  }

  [DllImport("user32.dll")]
  private static extern bool EnumWindows(EnumWindowsCallback callback, IntPtr argument);
  [DllImport("user32.dll")]
  private static extern bool EnumThreadWindows(uint threadId, EnumWindowsCallback callback, IntPtr argument);
  [DllImport("user32.dll")]
  private static extern bool EnumChildWindows(IntPtr parent, EnumWindowsCallback callback, IntPtr argument);
  [DllImport("user32.dll", SetLastError=true)]
  private static extern uint GetWindowThreadProcessId(IntPtr hwnd, out uint processId);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)]
  private static extern int GetClassName(IntPtr hwnd, StringBuilder buffer, int maxCount);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)]
  private static extern int GetWindowText(IntPtr hwnd, StringBuilder buffer, int maxCount);
  [DllImport("user32.dll")]
  private static extern bool IsWindowVisible(IntPtr hwnd);
  [DllImport("user32.dll")]
  private static extern bool IsWindowEnabled(IntPtr hwnd);
  [DllImport("user32.dll")]
  private static extern bool IsWindow(IntPtr hwnd);
  [DllImport("user32.dll")]
  private static extern IntPtr GetAncestor(IntPtr hwnd, uint flags);
  [DllImport("user32.dll")]
  private static extern int GetSystemMetrics(int index);
  [DllImport("user32.dll")]
  private static extern IntPtr GetWindow(IntPtr hwnd, int command);
  [DllImport("user32.dll")]
  private static extern IntPtr GetParent(IntPtr hwnd);
  [DllImport("user32.dll")]
  public static extern IntPtr GetDlgItem(IntPtr dialog, int itemId);
  [DllImport("user32.dll", EntryPoint="GetWindowLongPtrW", SetLastError=true)]
  private static extern IntPtr GetWindowLongPtr(IntPtr hwnd, int index);
  [DllImport("user32.dll", SetLastError=true)]
  private static extern bool GetGUIThreadInfo(uint threadId, ref GuiThreadInfo information);

  // Events are scoped to the exact QA app PID and this test process's
  // private desktop. Never observe or interact with the user's input desktop.
  private const uint EVENT_OBJECT_CREATE = 0x8000, EVENT_OBJECT_DESTROY = 0x8001;
  private delegate void WinEventCallback(IntPtr hook, uint ev, IntPtr hwnd,
    int objectId, int childId, uint eventThreadId, uint time);
  [DllImport("user32.dll", SetLastError=true)]
  private static extern IntPtr SetWinEventHook(uint first, uint last, IntPtr module,
    WinEventCallback callback, uint pid, uint tid, uint flags);
  [DllImport("user32.dll", SetLastError=true)]
  private static extern bool UnhookWinEvent(IntPtr hook);
  [StructLayout(LayoutKind.Sequential)]
  private struct Message {
    public IntPtr hwnd; public uint message; public IntPtr wParam, lParam;
    public uint time; public int x, y; public uint lPrivate;
  }
  [DllImport("user32.dll")]
  private static extern bool PeekMessageW(out Message msg, IntPtr hwnd,
    uint first, uint last, uint remove);
  [DllImport("user32.dll")]
  private static extern bool TranslateMessage(ref Message msg);
  [DllImport("user32.dll")]
  private static extern IntPtr DispatchMessageW(ref Message msg);
  [DllImport("user32.dll", EntryPoint="CreateWindowExW", CharSet=CharSet.Unicode, SetLastError=true)]
  private static extern IntPtr CreateNativeProbeWindow(uint exStyle, string cls, string title,
    uint style, int x, int y, int w, int h, IntPtr parent, IntPtr menu,
    IntPtr instance, IntPtr parameter);
  [DllImport("user32.dll")]
  private static extern bool DestroyWindow(IntPtr hwnd);
  [DllImport("user32.dll", SetLastError=true)]
  private static extern IntPtr GetThreadDesktop(uint threadId);
  // WM_GETTEXT is a system-marshaled Win32 message (below WM_USER). It can
  // retrieve the COMPLETE value of the verified, read-only native EDIT from a
  // different process even when UIA's provider exposes no text patterns.
  private const uint WM_GETTEXT = 0x000D, WM_GETTEXTLENGTH = 0x000E;
  private const uint BM_CLICK = 0x00F5;
  private const uint SMTO_BLOCK = 1, SMTO_ABORTIFHUNG = 2;
  [DllImport("user32.dll", EntryPoint="SendMessageTimeoutW", CharSet=CharSet.Unicode, SetLastError=true)]
  private static extern IntPtr GetNativeTextLength(IntPtr hwnd, uint msg,
    UIntPtr wp, IntPtr lp, uint flags, uint timeoutMs, out UIntPtr result);
  [DllImport("user32.dll", EntryPoint="SendMessageTimeoutW", CharSet=CharSet.Unicode, SetLastError=true)]
  private static extern IntPtr GetNativeText(IntPtr hwnd, uint msg,
    UIntPtr wp, StringBuilder buffer, uint flags, uint timeoutMs, out UIntPtr result);

  private static string FullNativeText(IntPtr hwnd) {
    UIntPtr nativeLength;
    if (GetNativeTextLength(hwnd, WM_GETTEXTLENGTH, UIntPtr.Zero, IntPtr.Zero,
       SMTO_BLOCK | SMTO_ABORTIFHUNG, 2500, out nativeLength) == IntPtr.Zero)
      throw new InvalidOperationException("Native text-length request failed/timed out: " + Marshal.GetLastWin32Error());
    ulong length = nativeLength.ToUInt64();
    if (length > 262144)
      throw new InvalidOperationException("Unbounded native review text is not acceptable");
    var buffer = new StringBuilder(checked((int)length + 2));
    UIntPtr copied;
    if (GetNativeText(hwnd, WM_GETTEXT, new UIntPtr(length + 2), buffer,
       SMTO_BLOCK | SMTO_ABORTIFHUNG, 2500, out copied) == IntPtr.Zero)
      throw new InvalidOperationException("Native text request failed/timed out: " + Marshal.GetLastWin32Error());
    string result = buffer.ToString();
    if ((ulong)result.Length != length || copied.ToUInt64() != length)
      throw new InvalidOperationException("Native read-only EDIT text was truncated or changed during retrieval");
    return result;
  }
  public static string FullNativeEditText(IntPtr hwnd) {
    AssertReadOnlyMultilineEdit(hwnd);
    return FullNativeText(hwnd);
  }
  public static void InvokeExactNativeButton(IntPtr dialog, IntPtr button,
    string expectedText) {
    if (!IsWindow(dialog) || !IsWindow(button)
        || !String.Equals(WindowClass(button), "Button", StringComparison.OrdinalIgnoreCase)
        || GetParent(button) != dialog || !IsWindowEnabled(button)
        || !IsWindowVisible(button))
      throw new InvalidOperationException("Native decision control is not a live, owned, enabled, visible Win32 BUTTON");
    if (FullNativeText(button) != expectedText)
      throw new InvalidOperationException("Native decision control text differs from exact fail-closed UI label");
    uint parentPid, buttonPid;
    GetWindowThreadProcessId(dialog, out parentPid);
    GetWindowThreadProcessId(button, out buttonPid);
    if (parentPid == 0 || parentPid != buttonPid)
      throw new InvalidOperationException("Native decision button PID differs from private consent PID");
    // Synthetic click on verified private native child only. This triggers the
    // button's real BN_CLICKED notification, which native Rust matches by
    // exact HWND. No physical desktop input, global focus or clipboard.
    UIntPtr result;
    if (GetNativeTextLength(button, BM_CLICK, UIntPtr.Zero, IntPtr.Zero,
        SMTO_BLOCK | SMTO_ABORTIFHUNG, 2500, out result) == IntPtr.Zero)
      throw new InvalidOperationException("Bounded native BM_CLICK failed/timed out: " + Marshal.GetLastWin32Error());
  }
  public static void InvokeReadIntentButton(IntPtr dialog, IntPtr button, int expectedId) {
    if (expectedId != YesId && expectedId != NoId)
      throw new InvalidOperationException("Only native YES/NO IDs are permitted");
    uint dialogPid, buttonPid;
    uint dialogThread = GetWindowThreadProcessId(dialog, out dialogPid);
    uint buttonThread = GetWindowThreadProcessId(button, out buttonPid);
    if (!IsWindow(dialog) || WindowClass(dialog) != ReadIntentClass
        || FullNativeText(dialog) != ReadIntentTitle || dialogThread == 0
        || button != GetDlgItem(dialog, expectedId) || GetParent(button) != dialog
        || !IsWindowVisible(button) || !IsWindowEnabled(button)
        || WindowClass(button) != "Button" || dialogPid != buttonPid
        || dialogThread != buttonThread)
      throw new InvalidOperationException("Cannot invoke a non-owned read-intent decision");
    UIntPtr result;
    if (GetNativeTextLength(button, BM_CLICK, UIntPtr.Zero, IntPtr.Zero,
        SMTO_BLOCK | SMTO_ABORTIFHUNG, 2500, out result) == IntPtr.Zero)
      throw new InvalidOperationException("Read-intent native BM_CLICK failed or timed out");
  }

  public static string SelfTestNativeEventHook(int testProcessId) {
    using (var observer = new PrivateNativeEventProbe(testProcessId)) {
      // Caller MUST first assert its own private non-input desktop. This
      // disposable native static window belongs only to the probe process.
      IntPtr hwnd = CreateNativeProbeWindow(0, "STATIC", "Private QA event test",
        0x10C00000, 10, 10, 160, 80, IntPtr.Zero, IntPtr.Zero,
        IntPtr.Zero, IntPtr.Zero);
      if (hwnd == IntPtr.Zero)
        throw new InvalidOperationException("Could not create private diagnostic window: " + Marshal.GetLastWin32Error());
      try {
        for (int i=0; i<40; i++) { observer.Pump(); System.Threading.Thread.Sleep(10); }
      } finally { DestroyWindow(hwnd); observer.Pump(); }
      if (observer.NativeControlsCreated + observer.OtherWindowEvents == 0)
        throw new InvalidOperationException("WinEventHook reported no CREATE/DESTROY events from same private process");
      return observer.Summary();
    }
  }

  public sealed class PrivateNativeEventProbe : IDisposable {
    private readonly WinEventCallback callback;
    private IntPtr hook;
    public int NativeCreateCount { get; private set; }
    public int NativeDestroyCount { get; private set; }
    public int NativeControlsCreated { get; private set; }
    public int OtherWindowEvents { get; private set; }
    public string LastFindTrace { get; private set; }
    private readonly HashSet<long> observedDialogHandles = new HashSet<long>();
    private readonly Dictionary<long, uint> dialogThreadIds = new Dictionary<long, uint>();
    public PrivateNativeEventProbe(int expectedPid) {
      LastFindTrace = "not_called";
      callback = Observe;
      // WINEVENT_OUTOFCONTEXT=0; all callbacks are pumped by this worker.
      hook = SetWinEventHook(EVENT_OBJECT_CREATE, EVENT_OBJECT_DESTROY,
        IntPtr.Zero, callback, (uint)expectedPid, 0, 0);
      if (hook == IntPtr.Zero)
        throw new InvalidOperationException("Could not install PID-scoped private native-window event observer: " + Marshal.GetLastWin32Error());
    }
    private void Observe(IntPtr source, uint ev, IntPtr hwnd,
      int objectId, int childId, uint threadId, uint time) {
      if (objectId != 0 || childId != 0 || hwnd == IntPtr.Zero) return;
      string cls = "";
      var name = new StringBuilder(128);
      if (GetClassName(hwnd, name, name.Capacity) != 0) cls = name.ToString();
      if (cls == ClassName) {
        if (ev == EVENT_OBJECT_CREATE) {
          NativeCreateCount++;
          observedDialogHandles.Add(hwnd.ToInt64());
          dialogThreadIds[hwnd.ToInt64()] = threadId;
        } else NativeDestroyCount++;
      } else if (cls == "Edit" || cls == "Button" || cls == "Static") {
        if (ev == EVENT_OBJECT_CREATE) NativeControlsCreated++;
      } else {
        OtherWindowEvents++;
      }
    }
    public void Pump() {
      Message msg;
      for (int n = 0; n < 128 && PeekMessageW(out msg, IntPtr.Zero, 0, 0, 1); n++) {
        TranslateMessage(ref msg);
        DispatchMessageW(ref msg);
      }
    }
    public string Summary() {
      return "native_created=" + NativeCreateCount + "; native_destroyed="
        + NativeDestroyCount + "; native_handles=" + observedDialogHandles.Count
        + "; native_controls_created=" + NativeControlsCreated
        + "; other_events=" + OtherWindowEvents;
    }
    public string NativeWindowDiagnostics(IntPtr expectedOwner) {
      var output = new StringBuilder();
      uint ownerPid;
      uint ownerThread = GetWindowThreadProcessId(expectedOwner, out ownerPid);
      IntPtr ownerDesktop = GetThreadDesktop(ownerThread);
      output.Append("owner_hwnd=").Append(expectedOwner.ToInt64())
        .Append("; owner_desktop=").Append(ownerDesktop.ToInt64());
      foreach (var kv in dialogThreadIds) {
        IntPtr hwnd = new IntPtr(kv.Key);
        IntPtr dialogDesktop = GetThreadDesktop(kv.Value);
        var title = new StringBuilder(256);
        var cls = new StringBuilder(96);
        if (IsWindow(hwnd)) {
          GetWindowText(hwnd, title, title.Capacity);
          GetClassName(hwnd, cls, cls.Capacity);
        }
        output.Append("; dialog_hwnd=").Append(kv.Key)
          .Append("; dialog_thread=").Append(kv.Value)
          .Append("; dialog_desktop=").Append(dialogDesktop.ToInt64())
          .Append("; same_desktop=").Append(dialogDesktop == ownerDesktop)
          .Append("; is_window=").Append(IsWindow(hwnd))
          .Append("; visible=").Append(IsWindowVisible(hwnd))
          .Append("; window_style=").Append(GetWindowLongPtr(hwnd, GWL_STYLE).ToInt64().ToString("X"))
          .Append("; parent=").Append(GetParent(hwnd).ToInt64())
          .Append("; class=").Append(cls)
          .Append("; title=").Append(title)
          .Append("; owner=").Append(GetWindow(hwnd, GW_OWNER).ToInt64())
          .Append("; same_owner=").Append(GetWindow(hwnd, GW_OWNER) == expectedOwner);
        int threadWindows = 0;
        EnumThreadWindows(kv.Value, (candidate, context) => {
          if (candidate == hwnd) threadWindows++;
          return true;
        }, IntPtr.Zero);
        output.Append("; enum_thread_contains_native=").Append(threadWindows == 1);
        foreach(int id in new [] { 10, 11, 12, 6, 7 }) {
          var child = GetDlgItem(hwnd, id);
          output.Append("; child_").Append(id).Append("=").Append(child.ToInt64());
        }
      }
      return output.ToString();
    }
    public IntPtr FindObservedOwnedVisible(IntPtr expectedOwner, int expectedPid) {
      uint ownerPid;
      uint ownerThread = GetWindowThreadProcessId(expectedOwner, out ownerPid);
      if (ownerThread == 0 || ownerPid != (uint)expectedPid)
        throw new InvalidOperationException("Observed native owner is no longer the private tested process");
      IntPtr ownerDesktop = GetThreadDesktop(ownerThread);
      if (ownerDesktop == IntPtr.Zero)
        throw new InvalidOperationException("Private owner desktop became unavailable");
      IntPtr match = IntPtr.Zero;
      LastFindTrace = "observed_native_handles=" + dialogThreadIds.Count;
      foreach (var pair in dialogThreadIds) {
        IntPtr candidate = new IntPtr(pair.Key);
        uint candidatePid;
        uint candidateTid = GetWindowThreadProcessId(candidate, out candidatePid);
        bool sameDesk = GetThreadDesktop(candidateTid) == ownerDesktop;
        bool live = IsWindow(candidate), visible = IsWindowVisible(candidate);
        LastFindTrace += "; hwnd=" + pair.Key + "; observed_tid=" + pair.Value
          + "; live_tid=" + candidateTid + "; candidate_pid=" + candidatePid
          + "; expected_pid=" + expectedPid + "; same_desktop=" + sameDesk
          + "; live=" + live + "; visible=" + visible;
        if (candidatePid != (uint)expectedPid || candidateTid != pair.Value
            || candidateTid == 0 || !sameDesk || !live || !visible) continue;
        var cls = new StringBuilder(96);
        var title = new StringBuilder(256);
        GetClassName(candidate, cls, cls.Capacity);
        GetWindowText(candidate, title, title.Capacity);
        LastFindTrace += "; class_eq=" + (cls.ToString() == ClassName)
          + "; title_eq=" + (title.ToString() == Title)
          + "; title_length=" + title.Length
          + "; owner_eq=" + (GetWindow(candidate, GW_OWNER) == expectedOwner);
        if (cls.ToString() != ClassName || title.ToString() != Title) continue;
        if (GetWindow(candidate, GW_OWNER) != expectedOwner)
          throw new InvalidOperationException("PID-bound native event dialog owner differs from exact private app window");
        if (match != IntPtr.Zero && match != candidate)
          throw new InvalidOperationException("More than one private native approval window observed");
        match = candidate;
      }
      return match;
    }
    public void Dispose() {
      if (hook != IntPtr.Zero) { UnhookWinEvent(hook); hook = IntPtr.Zero; }
    }
  }

  // Read-only owner state: exactly the Tauri HWND already verified to be on
  // the private desktop. Mirrors every early native InvalidWindow predicate.
  public static string OwnerDiagnostics(IntPtr hwnd, int expectedPid) {
    uint pid = 0;
    uint tid = GetWindowThreadProcessId(hwnd, out pid);
    return "is_window=" + IsWindow(hwnd)
      + "; owner_pid=" + pid + "; owner_tid=" + tid
      + "; expected_pid=" + expectedPid
      + "; root_matches=" + (GetAncestor(hwnd, 2) == hwnd)
      + "; enabled=" + IsWindowEnabled(hwnd)
      + "; screen_width=" + GetSystemMetrics(0)
      + "; screen_height=" + GetSystemMetrics(1)
      + "; class=" + WindowClass(hwnd);
  }

  public static IntPtr Find(int processId, IntPtr expectedOwner) {
    IntPtr found = IntPtr.Zero;
    int matches = 0;
    bool ownerMismatch = false;
    uint ownerPid;
    uint ownerThread = GetWindowThreadProcessId(expectedOwner, out ownerPid);
    if (ownerThread == 0 || ownerPid != (uint)processId)
      throw new InvalidOperationException("Native approval owner PID/thread changed during private UIA discovery");
    IntPtr ownerDesktop = GetThreadDesktop(ownerThread);
    if (ownerDesktop == IntPtr.Zero)
      throw new InvalidOperationException("Cannot identify private native approval owner desktop");
    // Tauri runs approval on a worker thread, not necessarily the window
    // owner's thread. Enumerate *every* GUI thread belonging to the exact
    // already-verified isolated process, on the verified owner desktop.
    // EnumWindows alone can omit owned non-input-desktop windows.
    using (var process = System.Diagnostics.Process.GetProcessById(processId)) {
      foreach (System.Diagnostics.ProcessThread thread in process.Threads) {
        uint candidateThread = (uint)thread.Id;
        if (GetThreadDesktop(candidateThread) != ownerDesktop) continue;
        EnumThreadWindows(candidateThread, (hwnd, arg) => {
          uint pid;
          uint liveThread = GetWindowThreadProcessId(hwnd, out pid);
          if (liveThread != candidateThread || pid != (uint)processId ||
              !IsWindowVisible(hwnd) || !IsWindow(hwnd)) return true;
          var cls = new StringBuilder(80);
          var title = new StringBuilder(256);
          GetClassName(hwnd, cls, cls.Capacity);
          GetWindowText(hwnd, title, title.Capacity);
          if (cls.ToString() == ClassName && title.ToString() == Title) {
            if (GetWindow(hwnd, GW_OWNER) != expectedOwner) ownerMismatch = true;
            else { found = hwnd; matches++; }
          }
          return true;
        }, IntPtr.Zero);
      }
    }
    if (ownerMismatch)
      throw new InvalidOperationException("Consent dialog owner differs from the isolated Tauri HWND");
    if (matches > 1) throw new InvalidOperationException("Ambiguous OS consent dialogs");
    return found;
  }

  // A standard MessageBoxW is NOT a V2 dialog. Find it by exact PID, desktop,
  // owner, class and caption. Refuse ambiguous or foreign matching windows.
  public static IntPtr FindReadIntent(int processId, IntPtr expectedOwner) {
    uint ownerPid;
    uint ownerThread = GetWindowThreadProcessId(expectedOwner, out ownerPid);
    if (ownerThread == 0 || ownerPid != (uint)processId)
      throw new InvalidOperationException("Read-intent owner PID/thread is no longer private");
    IntPtr ownerDesktop = GetThreadDesktop(ownerThread);
    if (ownerDesktop == IntPtr.Zero)
      throw new InvalidOperationException("Read-intent owner desktop unavailable");
    IntPtr found = IntPtr.Zero;
    using (var process = System.Diagnostics.Process.GetProcessById(processId)) {
      foreach (System.Diagnostics.ProcessThread thread in process.Threads) {
        uint tid = (uint)thread.Id;
        if (GetThreadDesktop(tid) != ownerDesktop) continue;
        EnumThreadWindows(tid, (hwnd, unused) => {
          uint pid;
          if (!IsWindow(hwnd) || !IsWindowVisible(hwnd)
              || GetWindowThreadProcessId(hwnd, out pid) != tid
              || pid != (uint)processId) return true;
          if (WindowClass(hwnd) != ReadIntentClass
              || FullNativeText(hwnd) != ReadIntentTitle) return true;
          if (GetWindow(hwnd, GW_OWNER) != expectedOwner)
            throw new InvalidOperationException("Read-intent MessageBox owner is not the private Tauri window");
          if (found != IntPtr.Zero && found != hwnd)
            throw new InvalidOperationException("Ambiguous private read-intent MessageBox windows");
          found = hwnd;
          return true;
        }, IntPtr.Zero);
      }
    }
    return found;
  }

  // Pure text assertions also serve as static helper self-tests: first-stage
  // consent must contain a lexical path and task, not inspected file metadata.
  public static void AssertReadIntentText(string text, string taskId, string relativePath) {
    if (String.IsNullOrEmpty(text) || String.IsNullOrEmpty(taskId)
        || String.IsNullOrEmpty(relativePath)
        || !text.Contains("FIRST permission request")
        || !text.Contains("No file must be opened, measured, or hashed before this decision.")
        || !text.Contains("Task ID: " + taskId)
        || !text.Contains("Exact relative path: \"" + relativePath + "\"")
        || !text.Contains("YES does NOT authorize reading contents into the app")
        || !text.Contains("A second native dialog must display the resolved path, byte count, and SHA-256.")
        || !text.Contains("NO is the default.")
        || System.Text.RegularExpressions.Regex.IsMatch(text,
          @"(?im)^[ \t]*(Canonical path|SHA-256|Byte count|Immutable operation digest|Operation ID)[ \t]*:"))
      throw new InvalidOperationException("Read-intent prompt is incomplete, unbound, or discloses inspected metadata");
    var taskLines = System.Text.RegularExpressions.Regex.Matches(text, @"(?m)^Task ID: ([^\r\n]+)\r?$");
    if (taskLines.Count != 1 || taskLines[0].Groups[1].Value != taskId)
      throw new InvalidOperationException("Read-intent prompt task must match one exact line");
    var pathLines = System.Text.RegularExpressions.Regex.Matches(text, @"(?m)^Exact relative path: ""([^\r\n]*)""\r?$");
    if (pathLines.Count != 1 || pathLines[0].Groups[1].Value != relativePath)
      throw new InvalidOperationException("Read-intent prompt path must match one exact lexical line");
  }

  public static string AssertReadIntentDialog(IntPtr dialog, IntPtr expectedOwner,
      int expectedPid, string taskId, string relativePath) {
    uint pid;
    uint tid = GetWindowThreadProcessId(dialog, out pid);
    if (tid == 0 || pid != (uint)expectedPid || !IsWindowVisible(dialog)
        || WindowClass(dialog) != ReadIntentClass
        || FullNativeText(dialog) != ReadIntentTitle
        || GetWindow(dialog, GW_OWNER) != expectedOwner)
      throw new InvalidOperationException("Read-intent MessageBox is not the expected owned native dialog");
    uint ownerPid;
    uint ownerTid = GetWindowThreadProcessId(expectedOwner, out ownerPid);
    if (ownerPid != pid || GetThreadDesktop(ownerTid) != GetThreadDesktop(tid))
      throw new InvalidOperationException("Read-intent dialog changed PID or desktop");
    IntPtr yes = GetDlgItem(dialog, YesId), no = GetDlgItem(dialog, NoId);
    if (yes == IntPtr.Zero || no == IntPtr.Zero || yes == no)
      throw new InvalidOperationException("Read-intent MessageBox does not expose both native decisions");
    foreach (IntPtr button in new [] { yes, no }) {
      uint buttonPid;
      if (GetParent(button) != dialog || !IsWindowEnabled(button) || !IsWindowVisible(button)
          || WindowClass(button) != "Button"
          || GetWindowThreadProcessId(button, out buttonPid) == 0 || buttonPid != pid)
        throw new InvalidOperationException("Read-intent decision is not an owned enabled native BUTTON");
    }
    UIntPtr defResult;
    if (GetNativeTextLength(dialog, DM_GETDEFID, UIntPtr.Zero, IntPtr.Zero,
          SMTO_BLOCK | SMTO_ABORTIFHUNG, 2500, out defResult) == IntPtr.Zero
        || (defResult.ToUInt64() >> 16) != DC_HASDEFID
        || (defResult.ToUInt64() & 0xffff) != NoId)
      throw new InvalidOperationException("Read-intent MessageBox does not default to NO");
    var info = new GuiThreadInfo();
    info.cbSize = (uint)Marshal.SizeOf(typeof(GuiThreadInfo));
    if (!GetGUIThreadInfo(tid, ref info)
        || info.hwndFocus != no || info.hwndActive != dialog)
      throw new InvalidOperationException("Read-intent MessageBox does not focus NO on its own GUI thread");
    var messages = new List<string>();
    EnumChildWindows(dialog, (child, unused) => {
      if (GetParent(child) == dialog && WindowClass(child) == "Static")
        messages.Add(FullNativeText(child));
      return true;
    }, IntPtr.Zero);
    string prompt = String.Join("\n", messages);
    AssertReadIntentText(prompt, taskId, relativePath);
    return prompt;
  }

  public static int AssertEightNativeControls(IntPtr dialog, int expectedPid) {
    uint pid;
    if (!IsWindow(dialog) || GetWindowThreadProcessId(dialog, out pid) == 0
        || pid != (uint)expectedPid || WindowClass(dialog) != ClassName
        || FullNativeText(dialog) != Title)
      throw new InvalidOperationException("Native V2 top-level window has invalid class, title or private process identity");
    var direct = new List<IntPtr>();
    EnumChildWindows(dialog, (child, arg) => {
      if (GetParent(child) == dialog) direct.Add(child);
      return true;
    }, IntPtr.Zero);
    if (direct.Count != 8)
      throw new InvalidOperationException("Native V2 dialog has " + direct.Count + " direct controls; exactly eight required");
    var expected = new Dictionary<int, string> {
      {10, "Edit"}, {11, "Edit"}, {12, "Edit"},
      {YesId, "Button"}, {NoId, "Button"}
    };
    foreach (var pair in expected) {
      IntPtr child = GetDlgItem(dialog, pair.Key);
      uint childPid;
      if (child == IntPtr.Zero || !direct.Contains(child)
          || GetWindowThreadProcessId(child, out childPid) == 0
          || childPid != (uint)expectedPid
          || !IsWindowVisible(child) || !IsWindowEnabled(child)
          || !String.Equals(WindowClass(child), pair.Value, StringComparison.OrdinalIgnoreCase))
        throw new InvalidOperationException("Missing or foreign native V2 child control ID " + pair.Key);
    }
    var labels = new HashSet<string>(StringComparer.Ordinal) {
      "Native consent. Read every field; NO is the default. Closing denies.",
      "Readable content preview: special characters are escaped (CreateNew only)",
      "Exact UTF-8 bytes, HEX with 8-digit offsets. Review all rows before YES."
    };
    foreach (IntPtr child in direct) {
      uint childPid;
      if (GetWindowThreadProcessId(child, out childPid) == 0
          || childPid != (uint)expectedPid || !IsWindowVisible(child))
        throw new InvalidOperationException("Native V2 includes a foreign or hidden direct child");
      string cls = WindowClass(child);
      if (String.Equals(cls, "Static", StringComparison.OrdinalIgnoreCase)) {
        if (!labels.Remove(FullNativeText(child)))
          throw new InvalidOperationException("Native V2 has an unexpected, repeated or altered static label");
      } else if (!expected.ContainsValue(cls)) {
        throw new InvalidOperationException("Native V2 has an unexpected direct child class");
      }
    }
    if (labels.Count != 0)
      throw new InvalidOperationException("Native V2 omitted or altered one of its three native safety labels");
    AssertReadOnlyMultilineEdit(GetDlgItem(dialog, 10));
    AssertReadOnlyMultilineEdit(GetDlgItem(dialog, 11));
    AssertReadOnlyMultilineEdit(GetDlgItem(dialog, 12));
    if (FullNativeText(GetDlgItem(dialog, YesId)) != "YES — approve this one operation" ||
        FullNativeText(GetDlgItem(dialog, NoId)) != "NO — deny (default)")
      throw new InvalidOperationException("Native V2 decision buttons do not contain the expected exact labels");
    return direct.Count;
  }

  public static string WindowClass(IntPtr hwnd) {
    var text = new StringBuilder(80);
    if (GetClassName(hwnd, text, text.Capacity) == 0) throw new InvalidOperationException("Cannot inspect native control class");
    return text.ToString();
  }

  public static void AssertDefaultNoAndFocus(IntPtr dialog, IntPtr yes, IntPtr no) {
    long noStyle = GetWindowLongPtr(no, GWL_STYLE).ToInt64();
    long yesStyle = GetWindowLongPtr(yes, GWL_STYLE).ToInt64();
    if ((noStyle & BS_TYPEMASK) != BS_DEFPUSHBUTTON || (yesStyle & BS_TYPEMASK) == BS_DEFPUSHBUTTON)
      throw new InvalidOperationException("Native No button lacks BS_DEFPUSHBUTTON or Yes is also default");
    uint pid;
    uint tid = GetWindowThreadProcessId(dialog, out pid);
    if (tid == 0) throw new InvalidOperationException("Cannot identify private dialog GUI thread");
    var info = new GuiThreadInfo();
    info.cbSize = (uint)Marshal.SizeOf(typeof(GuiThreadInfo));
    if (!GetGUIThreadInfo(tid, ref info))
      throw new InvalidOperationException("Cannot inspect keyboard focus on private consent GUI thread");
    if (info.hwndFocus != no || info.hwndActive != dialog)
      throw new InvalidOperationException("Native consent initial focus/active window is not No/isolated consent");
  }
  public static void AssertModalOwner(IntPtr owner, bool shouldBeEnabled) {
    if (owner == IntPtr.Zero || IsWindowEnabled(owner) != shouldBeEnabled)
      throw new InvalidOperationException("Owner HWND enabled state does not match the native modal lifecycle: hwnd="
        + owner.ToInt64() + ", actual=" + IsWindowEnabled(owner) + ", expected=" + shouldBeEnabled);
  }
  public static bool IsNativeWindow(IntPtr hwnd) { return IsWindow(hwnd); }
  public static bool IsEnabledWindow(IntPtr hwnd) { return IsWindowEnabled(hwnd); }

  public static void AssertReadOnlyMultilineEdit(IntPtr hwnd) {
    if (hwnd == IntPtr.Zero || !String.Equals(WindowClass(hwnd), "Edit", StringComparison.OrdinalIgnoreCase))
      throw new InvalidOperationException("Native review pane is not a Win32 EDIT control");
    long style = GetWindowLongPtr(hwnd, GWL_STYLE).ToInt64();
    if ((style & ES_READONLY) == 0 || (style & ES_MULTILINE) == 0
        || (style & WS_VSCROLL) == 0 || (style & WS_HSCROLL) == 0)
      throw new InvalidOperationException("Native review EDIT is not read-only, multiline and scrollable");
  }

  public static string FullEditValue(IntPtr hwnd) {
    var element = AutomationElement.FromHandle(hwnd);
    if (element == null) throw new InvalidOperationException("Cannot access native EDIT through UIA");
    object valueObject, textObject;
    bool hasValue = element.TryGetCurrentPattern(ValuePattern.Pattern, out valueObject);
    bool hasText = element.TryGetCurrentPattern(TextPattern.Pattern, out textObject);
    if (!hasValue && !hasText) {
      // Explicit Win32 fallback; still verify exact complete UTF-16 content,
      // immutable hash and byte-wise hex after return in the calling test.
      return FullNativeEditText(hwnd);
    }
    string viaValue = null, viaText = null;
    if (hasValue) {
      var value = (ValuePattern)valueObject;
      if (!value.Current.IsReadOnly)
        throw new InvalidOperationException("Native review content unexpectedly editable via UIA");
      viaValue = value.Current.Value;
    }
    if (hasText) viaText = ((TextPattern)textObject).DocumentRange.GetText(-1);
    if (hasValue && hasText && !String.Equals(viaValue.Replace("\r\n", "\n"), viaText.Replace("\r\n", "\n"), StringComparison.Ordinal))
      throw new InvalidOperationException("Native EDIT full ValuePattern and TextPattern disagree");
    return hasValue ? viaValue : viaText;
  }

  public static string ExactHex(byte[] bytes) {
    var result = new StringBuilder(bytes.Length * 4 + 128);
    for (int i = 0; i < bytes.Length; i += 16) {
      result.Append(i.ToString("X8", CultureInfo.InvariantCulture)).Append(": ");
      for (int j = i; j < Math.Min(i + 16, bytes.Length); j++)
        result.Append(bytes[j].ToString("X2", CultureInfo.InvariantCulture)).Append(" ");
      result.Append("\r\n");
    }
    return result.ToString();
  }

  // UI Automation InvokePattern on the app's WebView button may not return
  // until the native modal review window closes. Keep the same desktop process
  // responsive for discovering and deciding that dialog. This thread sends
  // no keyboard/mouse input, does not switch desktops and is bounded by the
  // test worker's kill-on-close Job Object.
  public static Task BeginWebViewInvoke(AutomationElement button) {
    return Task.Run(() => {
      object raw;
      if (!button.TryGetCurrentPattern(InvokePattern.Pattern, out raw))
        throw new InvalidOperationException("The proposed webview approval lacks InvokePattern");
      ((InvokePattern)raw).Invoke();
    });
  }
}
'@
  Add-Type -TypeDefinition $interop -ReferencedAssemblies 'System', 'UIAutomationClient', 'UIAutomationTypes' -ErrorAction Stop
}

# No application process, native window, filesystem target or input desktop is
# needed for this deterministic first-stage contract self-test.
function Test-ConstellationReadIntentHelperContract {
  $task = '11111111-2222-3333-4444-555555555555'
  $relative = 'notes/one-time.txt'
  $prompt = @"
FIRST permission request — inspect a named local file?
No file must be opened, measured, or hashed before this decision.
Task ID: $task
Exact relative path: "$relative"
YES permits inspection of this path for a later, separate native approval.
YES does NOT authorize reading contents into the app or sending them to a model.
A second native dialog must display the resolved path, byte count, and SHA-256.
NO is the default. Choose NO if you did not request this file.
"@
  [ConstellationPrivateConsentDialog]::AssertReadIntentText($prompt, $task, $relative)
  $bad = @(
    $prompt.Replace($task, '00000000-0000-0000-0000-000000000000'),
    $prompt.Replace($relative, 'notes/other.txt'),
    $prompt.Replace('NO is the default.', 'YES is the default.'),
    $prompt.Replace('No file must be opened, measured, or hashed before this decision.', 'File metadata was already inspected.'),
    ($prompt + "`nSHA-256: " + ('a' * 64)),
    ($prompt + "`nCanonical path: C:\private\secret.txt"),
    ($prompt + "`nByte count: 124"),
    ($prompt + "`nOperation ID: $task"),
    ($prompt + "`nExact relative path: `"$relative`"")
  )
  foreach ($candidate in $bad) {
    $rejected = $false
    try { [ConstellationPrivateConsentDialog]::AssertReadIntentText($candidate, $task, $relative) }
    catch { $rejected = $true }
    if (-not $rejected) { throw 'Read-intent self-test accepted a spoofed or metadata-bearing prompt.' }
  }
  [pscustomobject]@{
    test = 'two_stage_read_intent_pure_native_prompt'
    positive_checks = 1
    negative_checks = $bad.Count
    status = 'pass'
    active_desktop_touched = $false
    exe_launched = $false
  }
}

# The first read decision happens during PREPARE, before a renderer preview or
# any trusted file metadata inspection. This handler never reads the target
# file, passes credentials, or interacts with the input desktop.
function Invoke-ConstellationPrivateReadIntent {
  param(
    [Parameter(Mandatory=$true)]$Root,
    [Parameter(Mandatory=$true)][IntPtr]$ApplicationWindow,
    [Parameter(Mandatory=$true)][int]$ProcessId,
    [Parameter(Mandatory=$true)][string]$DesktopName,
    [Parameter(Mandatory=$true)][ValidateSet('No','Yes')][string]$Decision,
    [Parameter(Mandatory=$true)][string]$ExpectedTaskId,
    [Parameter(Mandatory=$true)][string]$ExpectedRelativePath,
    [string]$ExpectedPrivatePath,
    [string]$ExpectedFixtureSha256,
    [long]$ExpectedFixtureByteCount = -1,
    [ValidateSet('Request native permission to inspect', 'Review exact operation in native OS dialog')]
    [string]$RendererReviewLabel = 'Request native permission to inspect',
    [switch]$ExpectSecondNativeDialog,
    [ValidatePattern('^[a-z0-9-]{1,32}$')][string]$AttemptTag = 'first',
    [ValidateRange(3,25)][int]$TimeoutSeconds = 12
  )
  Assert-ConstellationHiddenDesktop $DesktopName
  Assert-ConstellationHiddenWindow $ApplicationWindow $DesktopName
  if ($ExpectedTaskId -notmatch '^[0-9a-fA-F]{8}(-[0-9a-fA-F]{4}){3}-[0-9a-fA-F]{12}$' -or
      $ExpectedRelativePath -notmatch '^[a-zA-Z0-9][a-zA-Z0-9._/-]*$' -or
      $ExpectedRelativePath -match '(^|/)\.\.?(/|$)|//|/$' -or
      $ExpectedRelativePath.Length -gt 240) {
    throw 'First-stage fixture task/path is invalid or unsafe; refusing native action.'
  }
  [ConstellationPrivateConsentDialog]::AssertModalOwner($ApplicationWindow, $true)
  $assertNoLeak = {
    $records = @(Ui-Records $Root)
    if (@($records | Where-Object { $_.Name -ceq 'Approval required for proposed file action' }).Count -ne 0) {
      throw 'A read action exposed a prepared renderer preview before first native permission.'
    }
    if ($ExpectedPrivatePath -and @($records | Where-Object {
      $_.Name -and $_.Name.Contains($ExpectedPrivatePath)
    }).Count -ne 0) {
      throw 'Read canonical file path leaked into WebView before initial permission.'
    }
    if ($ExpectedFixtureSha256 -and @($records | Where-Object {
      $_.Name -and $_.Name.Contains($ExpectedFixtureSha256)
    }).Count -ne 0) {
      throw 'Read fixture SHA-256 leaked into WebView before initial permission.'
    }
    if ($ExpectedFixtureByteCount -ge 0 -and @($records | Where-Object {
      $_.Name -match ('(?i)\b' + $ExpectedFixtureByteCount + '\s+bytes\s*·\s*SHA-256\b')
    }).Count -ne 0) {
      throw 'Read fixture byte count leaked into WebView before initial permission.'
    }
  }
  # Before and while the MessageBox blocks Rust preparation, check only the
  # renderer: this cannot prove no hidden filesystem calls in the binary.
  & $assertNoLeak
  # Guided multi-step and one-use prepare have different renderer labels;
  # either only summons a native dialog, never grants an inspection privilege.
  $submit = Ui-WaitEnabled $Root $RendererReviewLabel 8
  $invoke = [ConstellationPrivateConsentDialog]::BeginWebViewInvoke($submit.Element)
  $dialog = [IntPtr]::Zero
  $expires = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
  do {
    Assert-ConstellationHiddenDesktop $DesktopName
    $dialog = [ConstellationPrivateConsentDialog]::FindReadIntent($ProcessId, $ApplicationWindow)
    if ($dialog -ne [IntPtr]::Zero) { break }
    if ($invoke.IsFaulted) {
      throw 'UIA preparation failed before the first native read-intent permission.'
    }
    Start-Sleep -Milliseconds 125
  } while ([DateTime]::UtcNow -lt $expires)
  if ($dialog -eq [IntPtr]::Zero) {
    throw 'Expected first owned MessageBoxW read-intent dialog never appeared on the private desktop.'
  }
  Assert-ConstellationHiddenWindow $dialog $DesktopName
  [ConstellationPrivateConsentDialog]::AssertModalOwner($ApplicationWindow, $false)
  $observedPrompt = $null
  $focusDeadline = [DateTime]::UtcNow.AddSeconds(3)
  $focusError = 'No first-stage dialog focus observation'
  do {
    Assert-ConstellationHiddenDesktop $DesktopName
    try {
      $observedPrompt = [ConstellationPrivateConsentDialog]::AssertReadIntentDialog(
        $dialog, $ApplicationWindow, $ProcessId, $ExpectedTaskId, $ExpectedRelativePath)
      break
    } catch {
      $focusError = $_.Exception.GetBaseException().Message
      Start-Sleep -Milliseconds 100
    }
  } while ([DateTime]::UtcNow -lt $focusDeadline)
  if ($null -eq $observedPrompt) { throw "First native read-intent verification failed: $focusError" }
  & $assertNoLeak
  # The permission prompt itself must not contain the resolved filename, a
  # byte count or the SHA. The pure verifier rejects data-bearing field labels.
  $nativeScreenshot = $null
  if ($null -ne (Get-Command -Name Capture-Window -ErrorAction SilentlyContinue)) {
    # Do not overwrite the earlier first-YES screenshot when the read is
    # independently requested again for the TOCTOU mutation attempt.
    $nativeScreenshot = Capture-Window $dialog ('read-intent-' + $AttemptTag + '-' + $Decision.ToLowerInvariant())
  }
  $buttonId = if ($Decision -eq 'No') { 7 } else { 6 }
  $button = [ConstellationPrivateConsentDialog]::GetDlgItem($dialog, $buttonId)
  Assert-ConstellationHiddenWindow $button $DesktopName
  $buttonUi = [Windows.Automation.AutomationElement]::FromHandle($button)
  $pattern = $null
  if ($null -ne $buttonUi -and
      $buttonUi.Current.ControlType.ProgrammaticName -ceq 'ControlType.Button' -and
      $buttonUi.TryGetCurrentPattern([Windows.Automation.InvokePattern]::Pattern, [ref]$pattern)) {
    ([Windows.Automation.InvokePattern]$pattern).Invoke()
  } else {
    # Standard MessageBoxW buttons are native BUTTON controls. Avoid
    # label assertions here: button captions are localized by the OS.
    [ConstellationPrivateConsentDialog]::InvokeReadIntentButton($dialog, $button, $buttonId)
  }
  if ($ExpectSecondNativeDialog) {
    if ($Decision -ne 'Yes' -or $RendererReviewLabel -cne 'Review exact operation in native OS dialog') {
      throw 'Chained consent can only follow a real affirmative guided native read review.'
    }
    $nextDialog = [IntPtr]::Zero
    $nextDeadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
    do {
      Assert-ConstellationHiddenDesktop $DesktopName
      $nextDialog = [ConstellationPrivateConsentDialog]::Find($ProcessId, $ApplicationWindow)
      if ($nextDialog -ne [IntPtr]::Zero -and
          -not [ConstellationPrivateConsentDialog]::IsNativeWindow($dialog)) { break }
      if ($invoke.IsFaulted) { throw 'Original model-step click failed while awaiting separate OS confirmation' }
      Start-Sleep -Milliseconds 100
    } while ([DateTime]::UtcNow -lt $nextDeadline)
    if ($nextDialog -eq [IntPtr]::Zero -or [ConstellationPrivateConsentDialog]::IsNativeWindow($dialog)) {
      throw 'Read intent accepted but second exact OS-native consent did not appear.'
    }
    [ConstellationPrivateConsentDialog]::AssertModalOwner($ApplicationWindow, $false)
  } else {
    if (-not $invoke.Wait(7000)) { throw 'Renderer PREPARE invocation did not settle after native first-stage consent.' }
    if ($invoke.IsFaulted) { throw 'Renderer PREPARE UIA invocation failed after native first-stage consent.' }
  }
  $deadline = [DateTime]::UtcNow.AddSeconds(7)
  while ([DateTime]::UtcNow -lt $deadline -and
         ([ConstellationPrivateConsentDialog]::IsNativeWindow($dialog) -or
          (-not $ExpectSecondNativeDialog -and
           -not [ConstellationPrivateConsentDialog]::IsEnabledWindow($ApplicationWindow)))) {
    Assert-ConstellationHiddenDesktop $DesktopName
    Start-Sleep -Milliseconds 100
  }
  if ([ConstellationPrivateConsentDialog]::IsNativeWindow($dialog)) {
    throw 'First-stage native read-intent dialog did not close after explicit decision.'
  }
  Assert-ConstellationHiddenDesktop $DesktopName
  Assert-ConstellationHiddenWindow $ApplicationWindow $DesktopName
  [ConstellationPrivateConsentDialog]::AssertModalOwner($ApplicationWindow, (-not $ExpectSecondNativeDialog))
  return [pscustomobject]@{
    status='private_first_read_intent_verified'
    decision=$Decision
    task_id=$ExpectedTaskId
    relative_path=$ExpectedRelativePath
    default_button_id=7
    verified_no_file_metadata_in_native_prompt=$true
    screenshot=$nativeScreenshot
  }
}

function Invoke-ConstellationPrivateNativeConsent {
  param(
    [Parameter(Mandatory=$true)]$Root,
    [Parameter(Mandatory=$true)][IntPtr]$ApplicationWindow,
    [Parameter(Mandatory=$true)][int]$ProcessId,
    [Parameter(Mandatory=$true)][string]$DesktopName,
    [Parameter(Mandatory=$true)][ValidateSet('No','Yes')][string]$Decision,
    [Parameter(Mandatory=$true)][ValidateSet('create_new','read')][string]$Action,
    [Parameter(Mandatory=$true)][string]$ExpectedPath,
    [Parameter(Mandatory=$true)][string]$ExpectedTaskId,
    [string]$ExpectedContentSha256,
    [string]$ExpectedContent,
    [long]$ExpectedReadByteCount = -1,
    [ValidateSet('Approve this exact file action', 'Review exact operation in native OS dialog')]
    [string]$RendererApprovalLabel = 'Approve this exact file action',
    [switch]$ExistingNativeReviewInProgress,
    [ValidateRange(3,25)][int]$TimeoutSeconds = 12
  )
  Assert-ConstellationHiddenDesktop $DesktopName
  Assert-ConstellationHiddenWindow $ApplicationWindow $DesktopName
  [ConstellationPrivateConsentDialog]::AssertModalOwner($ApplicationWindow, (-not $ExistingNativeReviewInProgress))
  if ($ExpectedTaskId -notmatch '^[0-9a-fA-F]{8}(-[0-9a-fA-F]{4}){3}-[0-9a-fA-F]{12}$') {
    throw 'Cannot verify immutable task ID in OS confirmation.'
  }
  if (-not [IO.Path]::IsPathRooted($ExpectedPath)) {
    throw 'Cannot verify a relative path in OS confirmation.'
  }
  if ($ExpectedContentSha256 -notmatch '^[0-9a-fA-F]{64}$') {
    throw 'Expected bound file-content SHA-256 is missing.'
  }
  if ($Action -eq 'create_new') {
    if (-not $PSBoundParameters.ContainsKey('ExpectedContent')) {
      throw 'The original exact fixture contents must be supplied for native byte review.'
    }
    $bytes = [Text.Encoding]::UTF8.GetBytes($ExpectedContent)
    $sha = [BitConverter]::ToString([Security.Cryptography.SHA256]::Create().ComputeHash($bytes)).Replace('-', '')
    if ($sha -ine $ExpectedContentSha256) {
      throw 'Original fixture bytes disagree with the new-file digest.'
    }
  }
  if ($Action -eq 'read' -and $PSBoundParameters.ContainsKey('ExpectedContent')) {
    throw 'Read content must not be supplied to the native preview observer.'
  }
  if ($Action -eq 'read' -and ($ExpectedReadByteCount -lt 0 -or $ExpectedReadByteCount -gt 16 * 1024)) {
    throw 'Read consent requires a bounded, independently verified prepared byte count.'
  }
  $ownerBefore = [ConstellationPrivateConsentDialog]::OwnerDiagnostics($ApplicationWindow, $ProcessId)
  # Either workflow may REQUEST the dialog, but the renderer cannot grant
  # authority. The exact OS-native proof and journal one-use gate below are
  # identical and must still authenticate every operation independently.
  if ($ExistingNativeReviewInProgress -and
      ($Action -ne 'read' -or $RendererApprovalLabel -cne 'Review exact operation in native OS dialog')) {
    throw 'Chained native dialog may only verify a previously initiated read review.'
  }
  if (-not $ExistingNativeReviewInProgress) {
    $submit = Ui-WaitEnabled $Root $RendererApprovalLabel 8
  }
  $events = New-Object -TypeName 'ConstellationPrivateConsentDialog+PrivateNativeEventProbe' -ArgumentList @($ProcessId)
  try {
  $invoke = if ($ExistingNativeReviewInProgress) { $null } else {
    [ConstellationPrivateConsentDialog]::BeginWebViewInvoke($submit.Element)
  }
  $dialog = [IntPtr]::Zero
  $expires = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
  do {
    Assert-ConstellationHiddenDesktop $DesktopName
    $events.Pump()
    # Independently enumerate all exact-process threads on the owner's
    # private desktop; a Win32 owned popup can live on a Tauri worker thread.
    # The PID-bound event hook supplies a second observation. A disagreement
    # must fail closed before any native decision is delivered.
    $threadDialog = [ConstellationPrivateConsentDialog]::Find($ProcessId, $ApplicationWindow)
    $eventDialog = $events.FindObservedOwnedVisible($ApplicationWindow, $ProcessId)
    if ($threadDialog -ne [IntPtr]::Zero -and $eventDialog -ne [IntPtr]::Zero -and
        $threadDialog -ne $eventDialog) {
      throw 'Exact-process thread enumeration and native event hook identified different consent dialogs.'
    }
    $dialog = if ($threadDialog -ne [IntPtr]::Zero) { $threadDialog } else { $eventDialog }
    if ($dialog -ne [IntPtr]::Zero) { break }
    if ($null -ne $invoke -and $invoke.IsFaulted) {
      throw 'Private UIA approval action failed before the native OS confirmation appeared.'
    }
    Start-Sleep -Milliseconds 125
  } while ([DateTime]::UtcNow -lt $expires)
  if ($dialog -eq [IntPtr]::Zero) {
    $events.Pump()
    $ownerAfter = [ConstellationPrivateConsentDialog]::OwnerDiagnostics($ApplicationWindow, $ProcessId)
    $rendererStatus = @((Ui-Records $Root) | Where-Object {
      $_.Type -ceq 'ControlType.Text' -and $_.Name -match 'native confirmation|file operation was rejected|file action.*failed'
    } | Select-Object -First 3 -ExpandProperty Name)
    $nativeWindowDetails = $events.NativeWindowDiagnostics($ApplicationWindow)
    $postTimeoutFind = $events.FindObservedOwnedVisible($ApplicationWindow, $ProcessId)
    $threadFind = [ConstellationPrivateConsentDialog]::Find($ProcessId, $ApplicationWindow)
    throw "No owned ConstellationNativeConsentV2 window appeared on the private desktop; thread_find=$($threadFind.ToInt64()); post_timeout_event_find=$($postTimeoutFind.ToInt64()); find_trace=[$($events.LastFindTrace)]; native_events=[$($events.Summary())]; native_windows=[$nativeWindowDetails]; before=[$ownerBefore]; after=[$ownerAfter]; invocation_completed=$($invoke.IsCompleted); renderer_status=$($rendererStatus -join ' | ')."
  }
  Assert-ConstellationHiddenWindow $dialog $DesktopName
  [ConstellationPrivateConsentDialog]::AssertModalOwner($ApplicationWindow, $false)
  $verifiedNativeControls = [ConstellationPrivateConsentDialog]::AssertEightNativeControls($dialog, $ProcessId)
  $yes = [ConstellationPrivateConsentDialog]::GetDlgItem($dialog, 6)
  $no = [ConstellationPrivateConsentDialog]::GetDlgItem($dialog, 7)
  if ($yes -eq [IntPtr]::Zero -or $no -eq [IntPtr]::Zero) {
    throw 'Native consent window does not expose both owned Yes and No controls.'
  }
  Assert-ConstellationHiddenWindow $yes $DesktopName
  Assert-ConstellationHiddenWindow $no $DesktopName
  $ready = $false
  $focusError = 'No private GUI thread focus observation'
  $focusDeadline = [DateTime]::UtcNow.AddSeconds(3)
  do {
    Assert-ConstellationHiddenDesktop $DesktopName
    try {
      [ConstellationPrivateConsentDialog]::AssertDefaultNoAndFocus($dialog, $yes, $no)
      $ready = $true
      break
    } catch {
      $focusError = $_.Exception.GetBaseException().Message
      Start-Sleep -Milliseconds 100
    }
  } while ([DateTime]::UtcNow -lt $focusDeadline)
  if (-not $ready) {
    throw "Native V2 never established default-No keyboard focus on its isolated dialog: $focusError"
  }
  $detailsHandle = [ConstellationPrivateConsentDialog]::GetDlgItem($dialog, 10)
  $previewHandle = [ConstellationPrivateConsentDialog]::GetDlgItem($dialog, 11)
  $hexHandle = [ConstellationPrivateConsentDialog]::GetDlgItem($dialog, 12)
  foreach ($control in @($detailsHandle, $previewHandle, $hexHandle)) {
    Assert-ConstellationHiddenWindow $control $DesktopName
    [ConstellationPrivateConsentDialog]::AssertReadOnlyMultilineEdit($control)
  }
  $body = [ConstellationPrivateConsentDialog]::FullEditValue($detailsHandle)
  $preview = [ConstellationPrivateConsentDialog]::FullEditValue($previewHandle)
  $hex = [ConstellationPrivateConsentDialog]::FullEditValue($hexHandle)
  foreach ($required in @(
    'Constellation — native local file approval',
    'Operation ID:',
    'Task ID: ' + $ExpectedTaskId,
    'Action: ' + $Action,
    'Canonical path (complete):',
    $ExpectedPath,
    'Immutable operation digest:',
    'NO is selected by default.'
  )) {
    if (-not $body.Contains($required)) {
      throw 'Native read-only details pane omitted an immutable field or exact value.'
    }
  }
  # A substring is insufficient: an attacker could add a suffix to a preview
  # path (or UUID) and still satisfy Contains(). Bind complete native lines.
  $canonicalLine = [regex]::Match($body, '(?m)^Canonical path \(complete\):\r?\n([^\r\n]+)\r?$')
  if (-not $canonicalLine.Success -or $canonicalLine.Groups[1].Value -cne $ExpectedPath) {
    throw 'Native consent canonical path line differs from the exact immutable renderer preview.'
  }
  $taskLine = [regex]::Match($body, '(?m)^Task ID: ([^\r\n]+)\r?$')
  if (-not $taskLine.Success -or $taskLine.Groups[1].Value -cne $ExpectedTaskId) {
    throw 'Native consent task ID line differs from the saved pending task.'
  }
  $actionLine = [regex]::Match($body, '(?m)^Action: ([^\r\n]+)\r?$')
  if (-not $actionLine.Success -or $actionLine.Groups[1].Value -cne $Action) {
    throw 'Native consent action line differs from the previewed action.'
  }
  $operationMatch = [regex]::Match($body, '(?m)^Operation ID: ([0-9a-fA-F]{8}(-[0-9a-fA-F]{4}){3}-[0-9a-fA-F]{12})\r?$')
  if (-not $operationMatch.Success) {
    throw 'Native details pane did not bind a UUID-shaped immutable operation ID.'
  }
  $shaLine = [regex]::Match($body, '(?m)^SHA-256: ([0-9a-fA-F]{64})\r?$')
  if (-not $shaLine.Success -or $shaLine.Groups[1].Value -ine $ExpectedContentSha256) {
    throw 'Native read-only details pane SHA-256 differs from the immutable fixture snapshot.'
  }
  if ($Action -eq 'read') {
    # A caller-supplied byte count cannot count as observed native evidence.
    # Require the genuine native details pane to display and match the
    # privately prepared snapshot size before recording this check as passed.
    $byteLine = [regex]::Match($body, '(?m)^Byte count: ([0-9]+)\r?$')
    if (-not $byteLine.Success -or [long]$byteLine.Groups[1].Value -ne $ExpectedReadByteCount) {
      throw 'Native read consent did not display the exact private snapshot byte count.'
    }
  }
  if ($Action -eq 'read' -and -not $body.Contains('The executor rejects changed bytes')) {
    throw 'Native read consent omits the content-change rejection requirement.'
  }
  $digestMatch = [regex]::Match($body, '(?m)^Immutable operation digest: ([0-9a-fA-F]{64})\r?$')
  if (-not $digestMatch.Success) {
    throw 'Native details pane omitted the 64-hex immutable operation digest.'
  }
  if ($Action -eq 'create_new') {
    # Use the complete ValuePattern, never just the visible lines, a clipped
    # TextPattern range, the screen pixels, or a truncated accessibility Name.
    $expectedHex = [ConstellationPrivateConsentDialog]::ExactHex($bytes)
    if ($hex.Replace("`r`n", "`n") -cne $expectedHex.Replace("`r`n", "`n")) {
      throw 'Authoritative native 16-byte-row hex viewer is missing, altered or truncated.'
    }
    $expectedPreview = $ExpectedContent.Replace('\', '\\').Replace("`r", '\r').Replace("`n", "\n`r`n").Replace("`t", '\t')
    if ($preview.Replace("`r`n", "`n") -cne $expectedPreview.Replace("`r`n", "`n")) {
      throw 'Complete native readable preview does not match the immutable fixture text.'
    }
  } else {
    if ($hex -cne 'No content hex preview is available for a read. The executor verifies the pinned SHA-256 before returning content.' -or
        $preview -cne 'Read operation: SHA-256 snapshot is pinned. The file content is not displayed; only matching bytes may be returned after approval.') {
      throw 'Read confirmation did not expose the exact content-bound SHA-256 disclosure.'
    }
  }
  $nativeScreenshot = $null
  if ($null -ne (Get-Command -Name Capture-Window -ErrorAction SilentlyContinue)) {
    # Capture exactly the owned native V2 HWND (not the input desktop).
    $nativeScreenshot = Capture-Window $dialog ('native-v2-' + $Decision.ToLowerInvariant() + '-' + $Action + '-' + $operationMatch.Groups[1].Value.Substring(0,8))
  }
  $buttonHwnd = if ($Decision -eq 'No') { $no } else { $yes }
  $button = [Windows.Automation.AutomationElement]::FromHandle($buttonHwnd)
  $expectedButtonLabel = if ($Decision -eq 'No') { 'NO — deny (default)' } else { 'YES — approve this one operation' }
  Assert-ConstellationHiddenWindow $dialog $DesktopName
  $buttonPattern = $null
  if ($null -ne $button -and
      $button.Current.ControlType.ProgrammaticName -eq 'ControlType.Button' -and
      $button.TryGetCurrentPattern([Windows.Automation.InvokePattern]::Pattern, [ref]$buttonPattern)) {
    if ([string]$button.Current.Name -cne $expectedButtonLabel) {
      throw 'Native UIA decision control label differs from the immutable OS review action.'
    }
    ([Windows.Automation.InvokePattern]$buttonPattern).Invoke()
  } else {
    [ConstellationPrivateConsentDialog]::InvokeExactNativeButton($dialog, $buttonHwnd, $expectedButtonLabel)
  }
  if ($null -ne $invoke -and -not $invoke.Wait(7000)) {
    throw 'Original UIA approval activation did not complete after closing OS dialog.'
  }
  if ($null -ne $invoke -and $invoke.IsFaulted) {
    throw 'Original private UIA activation failed after consent dialog.'
  }
  # The UIA click task completes when the WebView click is dispatched, not
  # necessarily when the async Rust command has returned and dropped its
  # owner-enabling RAII guard. Observe the native modal lifetime explicitly.
  $modalDeadline = [DateTime]::UtcNow.AddSeconds(7)
  while ([DateTime]::UtcNow -lt $modalDeadline -and
         ([ConstellationPrivateConsentDialog]::IsNativeWindow($dialog) -or
          -not [ConstellationPrivateConsentDialog]::IsEnabledWindow($ApplicationWindow))) {
    Assert-ConstellationHiddenDesktop $DesktopName
    Start-Sleep -Milliseconds 100
  }
  if ([ConstellationPrivateConsentDialog]::IsNativeWindow($dialog)) {
    throw 'Native No/Yes button activation did not close its verified consent dialog within 7 seconds.'
  }
  Assert-ConstellationHiddenDesktop $DesktopName
  Assert-ConstellationHiddenWindow $ApplicationWindow $DesktopName
  [ConstellationPrivateConsentDialog]::AssertModalOwner($ApplicationWindow, $true)
  return [pscustomobject]@{
    status = 'private_native_v2_scrollable_review_verified'
    default_button_id = 7
    action = $Action
    decision = $Decision
    verified_owner = $true
    verified_complete_path = $true
    verified_task_id = $true
    verified_digest_format = $true
    verified_file_sha256 = $true
    verified_read_byte_count = if ($Action -eq 'read') { $ExpectedReadByteCount } else { $null }
    verified_readonly_three_panes = $true
    verified_native_control_count = $verifiedNativeControls
    verified_full_exact_hex_bytes = ($Action -eq 'create_new')
    verified_native_no_keyboard_focus = $true
    verified_modal_owner_lifecycle = $true
    operation_id = $operationMatch.Groups[1].Value.ToLowerInvariant()
    operation_digest = $digestMatch.Groups[1].Value.ToLowerInvariant()
    screenshot = $nativeScreenshot
  }
  } finally { $events.Dispose() }
}
