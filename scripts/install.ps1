# snow-cli install script (Windows)
# Usage: irm https://raw.githubusercontent.com/ewatch/snow-cli/main/scripts/install.ps1 | iex

$ErrorActionPreference = "Stop"

$Repo = "ewatch/snow-cli"

# --- Detect platform ---
$Arch = if ([System.Environment]::Is64BitOperatingSystem) { "x86_64" } else { "unknown" }
$Platform = "$Arch-pc-windows-msvc"

# --- Resolve install dir ---
$InstallDir = if ($env:INSTALL_DIR) { $env:INSTALL_DIR } else { "$env:LOCALAPPDATA\snow-cli\bin" }
if (!(Test-Path $InstallDir)) { New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null }

# --- Discover latest release ---
Write-Host "Checking latest release..."
$Release = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases/latest"
$Tag = $Release.tag_name
if (!$Tag) { Write-Error "Could not find latest release."; exit 1 }

$Archive = "snow-cli-$Platform.zip"
$Url = "https://github.com/$Repo/releases/download/$Tag/$Archive"

# --- Show plan ---
Write-Host ""
Write-Host "Plan:"
Write-Host "  Download: $Url"
Write-Host "  Release:  $Tag"
Write-Host "  Install to: $InstallDir"
Write-Host "  Binaries: snow-cli.exe, snow-cli-ro.exe"
Write-Host ""

# --- Confirm ---
if ($env:FORCE -ne "1") {
  $Reply = Read-Host "Proceed? [Y/n]"
  if ($Reply -match "^[Nn]") { Write-Host "Aborted."; exit 0 }
}

# --- Download & extract ---
$Tmp = New-TemporaryFile
$TmpDir = "$env:TEMP\snow-cli-install-$(Get-Random)"
New-Item -ItemType Directory -Path $TmpDir -Force | Out-Null

Write-Host "Downloading..."
Invoke-WebRequest -Uri $Url -OutFile "$Tmp.zip"

Write-Host "Extracting..."
Expand-Archive -Path "$Tmp.zip" -DestinationPath $TmpDir -Force

# --- Install binaries ---
@("snow-cli.exe", "snow-cli-ro.exe") | ForEach-Object {
  $Src = Get-ChildItem -Path $TmpDir -Name $_ -Recurse | Select-Object -First 1
  if (!$Src) {
    Write-Host "  Warning: $_ not found in archive."
  } else {
    $FullSrc = Join-Path $TmpDir $Src
    Copy-Item $FullSrc "$InstallDir\$_" -Force
    Write-Host "  Installed $_"
  }
}

# Cleanup
Remove-Item $Tmp -Force -ErrorAction SilentlyContinue
Remove-Item "$Tmp.zip" -Force -ErrorAction SilentlyContinue
Remove-Item $TmpDir -Recurse -Force -ErrorAction SilentlyContinue

# --- Post-install ---
Write-Host ""
Write-Host "Done."

$PathDirs = $env:PATH -split ";"
if ($PathDirs -notcontains $InstallDir) {
  Write-Host ""
  Write-Host "$InstallDir is not on your PATH. Add it with:"
  Write-Host "  [Environment]::SetEnvironmentVariable('Path', `$env:Path + ';$InstallDir', 'User')"
}

Write-Host ""
Write-Host "Verify: $InstallDir\snow-cli.exe --version"
