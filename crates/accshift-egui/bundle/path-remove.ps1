# Removes the install directory from the user PATH (path.nsh,
# ACCSHIFT_PATH_REMOVE), only when HKCU\Software\Accshift\Installer records
# it as added by the installer. A PATH entry that existed before installation
# is deliberately left untouched. The directory comes from the
# ACCSHIFT_INSTALL_DIR process variable, never from the command line.
$ErrorActionPreference = 'Stop'
try {
  $dir = [Environment]::GetEnvironmentVariable('ACCSHIFT_INSTALL_DIR', 'Process')
  if ([string]::IsNullOrWhiteSpace($dir)) { throw 'Missing install directory' }
  $m = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Software\Accshift\Installer', $true)
  if ($null -ne $m) {
    $owned = [string]$m.GetValue('PathEntry', '')
    if ([string]::Equals($owned, $dir, [StringComparison]::OrdinalIgnoreCase)) {
      $k = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Environment', $true)
      if ($null -eq $k) { throw 'Could not open user environment key' }
      $p = [string]$k.GetValue('Path', '', [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
      $n = $dir.TrimEnd('\')
      $all = @($p -split ';' | Where-Object { $_ })
      $e = @($all | Where-Object { -not [string]::Equals($_.TrimEnd('\'), $n, [StringComparison]::OrdinalIgnoreCase) })
      if ($e.Count -ne $all.Count) {
        $kind = if ($k.GetValueNames() -contains 'Path') { $k.GetValueKind('Path') } else { [Microsoft.Win32.RegistryValueKind]::ExpandString }
        $k.SetValue('Path', ($e -join ';'), $kind)
      }
      $k.Close()
      $m.DeleteValue('PathEntry', $false)
    }
    $m.Close()
  }
} catch {
  Write-Error $_
  exit 1
}
