# Native regression: installed-style Tauri executable, private Win32 desktop,
# first startup, then restart on the exact original unversioned SQLite DDL.
# All mutations stay in a freshly created disposable QA profile.
param(
  [Parameter(Mandatory=$true)][string]$ExePath,
  [int]$TimeoutSeconds = 25,
  [string]$HiddenDesktopWorkerName,
  [Parameter(Mandatory=$true)][ValidatePattern('^[a-fA-F0-9]{64}$')][string]$ExpectedExeSha256
)
$ErrorActionPreference='Stop'
$repo=Split-Path $PSScriptRoot -Parent
. (Join-Path $PSScriptRoot 'helper-hidden-desktop.ps1')
if(-not $HiddenDesktopWorkerName) {
  Invoke-ConstellationHiddenDesktopWorker -ScriptPath $PSCommandPath -ExePath $ExePath -ExpectedExeSha256 $ExpectedExeSha256 -TimeoutSeconds $TimeoutSeconds
  return
}
Assert-ConstellationHiddenDesktop $HiddenDesktopWorkerName
$exe=(Resolve-Path -LiteralPath $ExePath).ProviderPath
if((Get-FileHash -LiteralPath $exe -Algorithm SHA256).Hash -ine $ExpectedExeSha256) {throw 'Pinned executable hash mismatch'}
$qa=Join-Path (Join-Path $env:LOCALAPPDATA 'Temp') ('constellation-model-qa-'+[Guid]::NewGuid().ToString('N'))
$data=Join-Path $qa 'Data'
foreach($dir in @($qa,$data,(Join-Path $qa 'Temp'),(Join-Path $qa 'WebView2'))) {
  [void](New-Item -ItemType Directory -Path $dir -ErrorAction Stop)
}
$native=@'
using System;
using System.Text;
using System.Runtime.InteropServices;
public static class ConstellationStartupWindow {
  private delegate bool EnumProc(IntPtr hwnd, IntPtr unused);
  [DllImport("user32.dll")] private static extern bool EnumWindows(EnumProc cb, IntPtr unused);
  [DllImport("user32.dll")] private static extern uint GetWindowThreadProcessId(IntPtr hwnd, out uint pid);
  [DllImport("user32.dll",CharSet=CharSet.Unicode)] private static extern int GetWindowTextW(IntPtr hwnd,StringBuilder text,int size);
  public static bool HasMainWindow(int wanted) {
    bool found=false;
    EnumWindows((hwnd,unused)=>{
      uint pid;GetWindowThreadProcessId(hwnd,out pid);
      if(pid==(uint)wanted){
        var name=new StringBuilder(256);GetWindowTextW(hwnd,name,name.Capacity);
        if(name.ToString()=="Constellation"){found=true;return false;}
      }
      return true;
    },IntPtr.Zero);
    return found;
  }
}
'@
Add-Type -TypeDefinition $native -ErrorAction Stop
function Start-PrivateApp([switch]$Resume) {
  $flag=if($Resume){'--resume-isolated-data-dir'}else{'--isolated-data-dir'}
  $psi=New-Object Diagnostics.ProcessStartInfo
  $psi.FileName=$exe
  $psi.WorkingDirectory=Split-Path $exe -Parent
  $psi.Arguments=$flag+' "'+$data+'"'
  $psi.UseShellExecute=$false
  $psi.EnvironmentVariables['TEMP']=Join-Path $qa 'Temp'
  $psi.EnvironmentVariables['TMP']=Join-Path $qa 'Temp'
  $psi.EnvironmentVariables['WEBVIEW2_USER_DATA_FOLDER']=Join-Path $qa 'WebView2'
  $process=New-Object Diagnostics.Process
  $process.StartInfo=$psi
  if(-not $process.Start()){throw 'Launch failed'}
  try {
    $deadline=[DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
    do {
      $process.Refresh()
      if($process.HasExited){throw ('App exited before UI ready: '+$process.ExitCode)}
      if([ConstellationStartupWindow]::HasMainWindow($process.Id) -and
          (Test-Path -LiteralPath (Join-Path $data 'constellation.sqlite3') -PathType Leaf) -and
          (Test-Path -LiteralPath (Join-Path $data 'constellation-workflows.sqlite3') -PathType Leaf)) {
        Start-Sleep -Milliseconds 1500
        $process.Refresh()
        if($process.HasExited){throw ('App exited after opening its window: '+$process.ExitCode)}
        return $process
      }
      Start-Sleep -Milliseconds 250
    }while([DateTime]::UtcNow -lt $deadline)
    throw 'Main Constellation window did not appear on private desktop'
  } catch {
    if(-not $process.HasExited){$process.Kill();[void]$process.WaitForExit(5000)}
    $process.Dispose()
    throw
  }
}
function Stop-PrivateApp($process) {
  if(-not $process.HasExited){$process.Kill();[void]$process.WaitForExit(5000)}
  $process.Dispose()
}
$first=Start-PrivateApp
Stop-PrivateApp $first
$core=Join-Path $data 'constellation.sqlite3'
foreach($f in @((Join-Path $data '.constellation-owned-qa-v1'),$core,(Join-Path $data 'constellation-workflows.sqlite3'))) {
  if(-not (Test-Path -LiteralPath $f -PathType Leaf)){throw ('First-run QA file missing: '+$f)}
}
'NATIVE_FIRST_LAUNCH_PASS'
$fixture=@'
const fs=require("fs"),{DatabaseSync}=require("node:sqlite");
const dbpath=process.argv[2],source=process.argv[3];
if(!dbpath.includes("constellation-model-qa-")||!dbpath.endsWith("constellation.sqlite3"))throw Error("Not a disposable QA file");
const ddl=fs.readFileSync(source,"utf8").split('const STORE_LEGACY_SCHEMA_DDL: &str = "')[1].split('";')[0];
if(!ddl||!ddl.includes("CREATE TABLE IF NOT EXISTS rooms"))throw Error("Historic schema unavailable");
const active=new DatabaseSync(dbpath);active.exec("PRAGMA wal_checkpoint(TRUNCATE)");active.close();
for(const suffix of ["","-wal","-shm"]){
 const f=dbpath+suffix;if(fs.existsSync(f))fs.renameSync(f,f+".first-run-preserved");
}
const legacy=new DatabaseSync(dbpath);legacy.exec(ddl);
if(legacy.prepare("PRAGMA user_version").get().user_version!==0)throw Error("Historic version not zero");
legacy.close();
'@
$fixture | node - $core (Join-Path $repo 'crates\constellation-core\src\store.rs')
if($LASTEXITCODE -ne 0){throw 'Failed to set up exact historic fixture'}
$reopened=Start-PrivateApp -Resume
Stop-PrivateApp $reopened
$verify=@'
const {DatabaseSync}=require("node:sqlite");
const db=new DatabaseSync(process.argv[2],{readOnly:true});
const version=db.prepare("PRAGMA user_version").get().user_version;
const integrity=db.prepare("PRAGMA quick_check(1)").get().quick_check;
console.log("MIGRATED_USER_VERSION="+version+" INTEGRITY="+integrity);
if(version!==1||integrity!=="ok")process.exitCode=1;
db.close();
'@
$verify|node - $core
if($LASTEXITCODE -ne 0){throw 'Historic schema migration did not survive a real native launch'}
'NATIVE_HISTORIC_REOPEN_PASS; REAL_USER_PROFILE_UNTOUCHED'
