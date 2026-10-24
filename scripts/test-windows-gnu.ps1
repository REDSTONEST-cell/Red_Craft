param(
    [string]$CargoPath = 'cargo',
    [Parameter(Mandatory = $true)][string]$GnuBin
)
$ErrorActionPreference = 'Stop'
# GNU Tauri test harnesses need the Common Controls v6 activation context.
# This affects generated test executables only, never Windows or installed apps.
# Keep matching GCC DLLs first; Rust's bundled libgcc can otherwise shadow them.
$previousPath = $env:Path
try {
    $env:Path = "$GnuBin;$previousPath"
    $artifacts = & $CargoPath test --workspace --no-run --message-format=json
    if ($LASTEXITCODE -ne 0) { throw 'Rust test compilation failed' }
    if (-not ('RedCraftTestManifest' -as [type])) {
        Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public class RedCraftTestManifest {
    [DllImport("kernel32", CharSet=CharSet.Unicode, SetLastError=true)]
    public static extern IntPtr BeginUpdateResource(string path, bool deleteExisting);
    [DllImport("kernel32", SetLastError=true)]
    public static extern bool UpdateResource(IntPtr handle, IntPtr type, IntPtr name, ushort language, byte[] bytes, uint size);
    [DllImport("kernel32", SetLastError=true)]
    public static extern bool EndUpdateResource(IntPtr handle, bool discard);
}
'@
    }
    $manifest = [Text.Encoding]::UTF8.GetBytes(@'
<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <dependency><dependentAssembly><assemblyIdentity type="win32" name="Microsoft.Windows.Common-Controls" version="6.0.0.0" processorArchitecture="*" publicKeyToken="6595b64144ccf1df" language="*" /></dependentAssembly></dependency>
</assembly>
'@)
    foreach ($line in $artifacts) {
        $artifact = $line | ConvertFrom-Json
        if ($artifact.reason -ne 'compiler-artifact' -or -not $artifact.profile.test -or -not $artifact.executable) { continue }
        $handle = [RedCraftTestManifest]::BeginUpdateResource($artifact.executable, $false)
        if ($handle -eq [IntPtr]::Zero) { throw "Cannot open test manifest: $($artifact.executable)" }
        if (-not [RedCraftTestManifest]::UpdateResource($handle, [IntPtr]24, [IntPtr]1, 1033, $manifest, $manifest.Length)) {
            [void][RedCraftTestManifest]::EndUpdateResource($handle, $true)
            throw "Cannot update test manifest: $($artifact.executable)"
        }
        if (-not [RedCraftTestManifest]::EndUpdateResource($handle, $false)) { throw 'Cannot save test manifest' }
    }
    & $CargoPath test --workspace
    if ($LASTEXITCODE -ne 0) { throw 'Rust tests failed' }
} finally { $env:Path = $previousPath }
