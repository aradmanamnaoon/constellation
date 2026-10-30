# Verify a real installed Tauri application without showing windows on the
# user's interactive desktop. Creates a separate, unselected Win32 desktop,
# starts the EXE there, walks its native UI Automation tree and terminates only
# that isolated process. Does NOT call SwitchDesktop or foreground APIs.
param(
  [Parameter(Mandatory=$true)][string]$Executable,
  [int]$TimeoutSeconds = 25,
  [switch]$TryInteractiveUia
)
$ErrorActionPreference = 'Stop'
if (-not $IsWindows -and $env:OS -ne 'Windows_NT') { throw 'Requires Windows' }
$Executable = (Resolve-Path -LiteralPath $Executable).Path
if (-not $Executable.EndsWith('.exe', [StringComparison]::OrdinalIgnoreCase)) {throw 'Expected an executable'}

$source = @'
using System;
using System.Diagnostics;
using System.ComponentModel;
using System.Runtime.InteropServices;
using System.Text;
using System.Threading;
using System.Windows.Automation;

public static class ConstellationHiddenDesktop {
  [StructLayout(LayoutKind.Sequential, CharSet=CharSet.Unicode)]
  private struct STARTUPINFO {
    public int cb; public string lpReserved; public string lpDesktop; public string lpTitle;
    public int dwX; public int dwY; public int dwXSize; public int dwYSize;
    public int dwXCountChars; public int dwYCountChars; public int dwFillAttribute;
    public int dwFlags; public short wShowWindow; public short cbReserved2;
    public IntPtr lpReserved2; public IntPtr hStdInput; public IntPtr hStdOutput; public IntPtr hStdError;
  }
  [StructLayout(LayoutKind.Sequential)]
  private struct PROCESS_INFORMATION {
    public IntPtr hProcess; public IntPtr hThread; public int dwProcessId; public int dwThreadId;
  }
  [DllImport("user32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
  private static extern IntPtr CreateDesktop(string name, string device, IntPtr devmode, int flags, uint desiredAccess, IntPtr securityAttributes);
  [DllImport("user32.dll", SetLastError=true)]
  private static extern bool CloseDesktop(IntPtr desktop);
  [DllImport("user32.dll", SetLastError=true)]
  private static extern bool SetThreadDesktop(IntPtr desktop);
  [DllImport("user32.dll", SetLastError=true)]
  private static extern IntPtr GetForegroundWindow();
  private delegate bool EnumDesktopWindowsProc(IntPtr hwnd, IntPtr param);
  [DllImport("user32.dll", SetLastError=true)]
  private static extern bool EnumDesktopWindows(IntPtr desktop, EnumDesktopWindowsProc callback, IntPtr param);
  [DllImport("user32.dll", SetLastError=true)]
  private static extern uint GetWindowThreadProcessId(IntPtr hwnd, out uint processId);
  [DllImport("user32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
  private static extern int GetWindowText(IntPtr hwnd, StringBuilder buffer, int maxCount);
  [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
  private static extern bool CreateProcess(string applicationName, string commandLine, IntPtr processAttributes, IntPtr threadAttributes,
    bool inheritHandles, uint creationFlags, IntPtr environment, string currentDirectory, ref STARTUPINFO startup, out PROCESS_INFORMATION process);
  [DllImport("kernel32.dll", SetLastError=true)]
  private static extern bool CloseHandle(IntPtr handle);

  public static string Run(string executable, int waitSeconds, bool interactiveUia) {
    if (waitSeconds < 2 || waitSeconds > 90) throw new ArgumentOutOfRangeException("waitSeconds");
    IntPtr foregroundBefore = GetForegroundWindow();
    string name = "ConstellationQA_" + Guid.NewGuid().ToString("N");
    // DESKTOP_CREATEWINDOW | DESKTOP_ENUMERATE | DESKTOP_READOBJECTS |
    // DESKTOP_WRITEOBJECTS | DESKTOP_SWITCHDESKTOP | READ_CONTROL
    // (only the current process holds this handle; we NEVER switch desktops).
    IntPtr desktop = CreateDesktop(name, null, IntPtr.Zero, 0, 0x000F01FF, IntPtr.Zero);
    if (desktop == IntPtr.Zero) throw new Win32Exception(Marshal.GetLastWin32Error(), "CreateDesktop failed");
    PROCESS_INFORMATION spawned = default(PROCESS_INFORMATION);
    Process app = null;
    try {
      var startup = new STARTUPINFO { cb=Marshal.SizeOf(typeof(STARTUPINFO)), lpDesktop="WinSta0\\" + name, dwFlags=1, wShowWindow=0 };
      if (!CreateProcess(executable, "\""+executable+"\"", IntPtr.Zero, IntPtr.Zero, false, 0, IntPtr.Zero,
          System.IO.Path.GetDirectoryName(executable), ref startup, out spawned)) {
        throw new Win32Exception(Marshal.GetLastWin32Error(), "CreateProcess on hidden desktop failed");
      }
      CloseHandle(spawned.hThread); spawned.hThread = IntPtr.Zero;
      CloseHandle(spawned.hProcess); spawned.hProcess = IntPtr.Zero;
      app = Process.GetProcessById(spawned.dwProcessId);
      IntPtr hwnd = IntPtr.Zero;
      string title = "";
      int count = 0;
      string[] names = new string[0];
      string[] visibleLabels = new string[0];
      string interaction = "not-run: read-only hidden-desktop smoke";
      var deadline = DateTime.UtcNow.AddSeconds(waitSeconds);
      while (DateTime.UtcNow < deadline && !app.HasExited) {
        EnumDesktopWindows(desktop, (window, p) => {
          uint pid; GetWindowThreadProcessId(window, out pid);
          if (pid == (uint)spawned.dwProcessId) {
            var buf = new StringBuilder(512); GetWindowText(window, buf, buf.Capacity);
            // Tauri creates several helper HWNDs (notably "Tooltip") before
            // the actual app window; never mistake those for the primary UI.
            if (buf.ToString().IndexOf("Constellation", StringComparison.OrdinalIgnoreCase) >= 0) {
              hwnd = window; title = buf.ToString(); return false;
            }
          }
          return true;
        }, IntPtr.Zero);
        if (hwnd != IntPtr.Zero) {
          var completed = new ManualResetEventSlim(false);
          Exception error = null;
          // Attach a fresh thread BEFORE it touches any windows/automation.
          var thread = new Thread(() => {
            try {
              if (!SetThreadDesktop(desktop)) throw new Win32Exception(Marshal.GetLastWin32Error(), "SetThreadDesktop failed");
              var element = AutomationElement.FromHandle(hwnd);
              var descendants = element.FindAll(TreeScope.Descendants, Condition.TrueCondition);
              count = descendants.Count;
              var matched = new System.Collections.Generic.List<string>();
              var observed = new System.Collections.Generic.List<string>();
              foreach (AutomationElement child in descendants) {
                var label = child.Current.Name;
                if (!string.IsNullOrEmpty(label) && observed.Count < 25) observed.Add(label);
                if (label == "Rooms" || label == "Tasks" || label == "Settings" || label == "Library" || label == "API FORMAT") matched.Add(label);
              }
              names = matched.ToArray();
              visibleLabels = observed.ToArray();
            } catch (Exception e) { error = e; } finally { completed.Set(); }
          });
          thread.IsBackground = true; thread.SetApartmentState(ApartmentState.STA); thread.Start();
          if (completed.Wait(3500) && error == null && count >= 40 && names.Length >= 3) break;
          hwnd = IntPtr.Zero;
        }
        Thread.Sleep(250);
      }
      // Cross-desktop UIA InvokePattern may hang inside Windows/WebView2.
      // Disabled by default; never force interaction on the visible desktop.
      if (interactiveUia && hwnd != IntPtr.Zero && count >= 40) {
        var checkedUi = new ManualResetEventSlim(false);
        var interactionThread = new Thread(() => {
          try {
            if (!SetThreadDesktop(desktop)) throw new Win32Exception(Marshal.GetLastWin32Error());
            var root = AutomationElement.FromHandle(hwnd);
            Func<string, AutomationElement> find = label => root.FindFirst(
              TreeScope.Descendants, new PropertyCondition(AutomationElement.NameProperty, label));
            var settings = find("Settings");
            if (settings == null) throw new Exception("settings button not found");
            ((InvokePattern)settings.GetCurrentPattern(InvokePattern.Pattern)).Invoke();
            Thread.Sleep(250);
            var add = find("Add provider");
            if (add == null) throw new Exception("Add provider button not found");
            ((InvokePattern)add.GetCurrentPattern(InvokePattern.Pattern)).Invoke();
            Thread.Sleep(250);
            var formatElements = root.FindAll(TreeScope.Descendants,
              new PropertyCondition(AutomationElement.NameProperty,"API FORMAT"));
            AutomationElement protocol = null;
            foreach (AutomationElement candidate in formatElements)
              if (candidate.Current.ControlType == ControlType.ComboBox) { protocol = candidate; break; }
            if (protocol == null)
              throw new Exception("six-provider protocol selector absent");
            var modal = find("Connect AI provider");
            if (modal == null) throw new Exception("provider modal absent");
            var close = find("Close dialog");
            if (close == null) throw new Exception("provider modal close absent");
            ((InvokePattern)close.GetCurrentPattern(InvokePattern.Pattern)).Invoke();
            Thread.Sleep(100);
            var library = find("Library");
            ((InvokePattern)library.GetCurrentPattern(InvokePattern.Pattern)).Invoke();
            Thread.Sleep(250);
            if (find("Search agent roles") == null) throw new Exception("role search missing");
            var tasks = find("Tasks");
            ((InvokePattern)tasks.GetCurrentPattern(InvokePattern.Pattern)).Invoke();
            Thread.Sleep(250);
            if (find("Run text task") == null && find("Create task") == null && find("New task") == null)
              throw new Exception("task controls not accessible");
            interaction = "settings/provider-dialog/library/tasks passed";
          } catch (Exception e) {
            interaction = "failed: "+e.GetType().Name+": "+e.Message;
          } finally { checkedUi.Set(); }
        });
        interactionThread.IsBackground = true;
        interactionThread.SetApartmentState(ApartmentState.STA);
        interactionThread.Start();
        if (!checkedUi.Wait(8500)) interaction = "failed: UIA interaction timed out";
      }
      bool unchanged = foregroundBefore == GetForegroundWindow();
      return "pid="+spawned.dwProcessId+"; title="+title+"; uia_descendants="+count+
        "; expected_controls="+string.Join(",",names)+"; foreground_unchanged="+unchanged+
        "; process_alive="+(!app.HasExited)+"; hidden_desktop="+name+
        "; sample_labels="+string.Join("|",visibleLabels)+
        "; interaction="+interaction;
    } finally {
      if (app != null) {
        try { if (!app.HasExited) { app.Kill(); app.WaitForExit(5000); } } catch {}
        app.Dispose();
      }
      if (spawned.hThread != IntPtr.Zero) CloseHandle(spawned.hThread);
      if (spawned.hProcess != IntPtr.Zero) CloseHandle(spawned.hProcess);
      CloseDesktop(desktop);
    }
  }
}
'@
Add-Type -TypeDefinition $source -ReferencedAssemblies 'UIAutomationClient','UIAutomationTypes','System.Core','System' -ErrorAction Stop
$outcome=[ConstellationHiddenDesktop]::Run($Executable,$TimeoutSeconds,[bool]$TryInteractiveUia)
Write-Output $outcome
if ($outcome -notmatch 'expected_controls=.*Rooms.*Tasks.*Library.*Settings' -or $outcome -notmatch 'foreground_unchanged=True' -or ($TryInteractiveUia -and $outcome -notmatch 'interaction=settings/provider-dialog/library/tasks passed')) { throw 'Hidden desktop GUI verification failed; do not claim success' }
