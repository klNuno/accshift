# Adds the install directory to the user PATH (path.nsh, ACCSHIFT_PATH_ADD).
# The directory comes from the ACCSHIFT_INSTALL_DIR process variable, never
# from the command line: install directories may legally contain apostrophes
# or other shell metacharacters. Records the entry under
# HKCU\Software\Accshift\Installer so the uninstaller removes only what this
# installer added.
$ErrorActionPreference = 'Stop'
try {
  $dir = [Environment]::GetEnvironmentVariable('ACCSHIFT_INSTALL_DIR', 'Process')
  if ([string]::IsNullOrWhiteSpace($dir) -or $dir.Contains(';')) { throw 'Invalid install directory' }
  $k = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Environment', $true)
  if ($null -eq $k) { throw 'Could not open user environment key' }
  $p = [string]$k.GetValue('Path', '', [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
  $n = $dir.TrimEnd('\')
  $exists = @($p -split ';' | Where-Object { $_ -and [string]::Equals($_.TrimEnd('\'), $n, [StringComparison]::OrdinalIgnoreCase) }).Count -gt 0
  $m = [Microsoft.Win32.Registry]::CurrentUser.CreateSubKey('Software\Accshift\Installer')
  $owned = [string]$m.GetValue('PathEntry', '')
  if (-not $exists) {
    $kind = if ($k.GetValueNames() -contains 'Path') { $k.GetValueKind('Path') } else { [Microsoft.Win32.RegistryValueKind]::ExpandString }
    $k.SetValue('Path', ($p.TrimEnd(';') + ';' + $dir).TrimStart(';'), $kind)
    $m.SetValue('PathEntry', $dir, [Microsoft.Win32.RegistryValueKind]::String)
  } elseif (-not [string]::Equals($owned, $dir, [StringComparison]::OrdinalIgnoreCase)) {
    # An entry that existed before installation is not ours to remove later.
    $m.DeleteValue('PathEntry', $false)
  }
  $m.Close()
  $k.Close()
} catch {
  Write-Error $_
  exit 1
}
