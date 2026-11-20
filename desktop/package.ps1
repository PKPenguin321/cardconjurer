<#
.SYNOPSIS
    Builds Card Conjurer (release, no console window) and assembles a portable folder.

.DESCRIPTION
    Output: desktop\release\Card Conjurer\
        Card Conjurer.exe
        assets\            (only the folders the creator uses)

    The app finds its files in the "assets" folder next to the exe, so the
    whole "Card Conjurer" folder can be moved, zipped or shared as-is.
    Saved cards are kept separately, in %APPDATA%\com.cardconjurer.desktop\cards.

.PARAMETER Zip
    Also create desktop\release\Card Conjurer.zip.

.PARAMETER SkipBuild
    Reuse the existing release exe and only refresh the assets.

.EXAMPLE
    .\package.ps1
    .\package.ps1 -Zip
#>
param(
    [switch]$Zip,
    [switch]$SkipBuild
)

$ErrorActionPreference = 'Stop'

$desktop = $PSScriptRoot
$repo    = Split-Path $desktop -Parent
$tauri   = Join-Path $desktop 'src-tauri'
$release = Join-Path $desktop 'release'
$appDir  = Join-Path $release 'Card Conjurer'
$assets  = Join-Path $appDir 'assets'
$exeName = 'Card Conjurer.exe'

# Folders the creator needs, relative to the repo root.
$assetFolders = @('css', 'js', 'img', 'fonts', 'creator', 'core', 'local_art', 'desktop\web')

# 1. Build. Release builds use the Windows GUI subsystem (see main.rs), so no console.
if (-not $SkipBuild) {
    Write-Host 'Building release exe...' -ForegroundColor Cyan
    cargo build --release --manifest-path (Join-Path $tauri 'Cargo.toml')
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed with exit code $LASTEXITCODE" }
}
$builtExe = Join-Path $tauri 'target\release\cardconjurer-desktop.exe'
if (-not (Test-Path $builtExe)) { throw "Release exe not found at $builtExe. Run without -SkipBuild." }

# 2. Exe.
New-Item -ItemType Directory -Force -Path $assets | Out-Null
Copy-Item $builtExe (Join-Path $appDir $exeName) -Force

# 3. Assets. robocopy /MIR mirrors each folder, copying only what changed,
#    and removes files that no longer exist in the repo.
foreach ($folder in $assetFolders) {
    $src = Join-Path $repo $folder
    $dst = Join-Path $assets $folder
    if (-not (Test-Path $src)) { Write-Warning "Skipping missing folder $folder"; continue }
    Write-Host "Copying $folder..." -ForegroundColor Cyan
    robocopy $src $dst /MIR /NFL /NDL /NJH /NJS /NP /R:1 /W:1 | Out-Null
    # robocopy exit codes 0-7 mean success; 8 and up mean failure.
    if ($LASTEXITCODE -ge 8) { throw "Copying $folder failed (robocopy exit code $LASTEXITCODE)" }
}
# Remove folders from earlier runs that are no longer in the list.
$keep = $assetFolders | ForEach-Object { ($_ -split '\\')[0] }
Get-ChildItem $assets -Directory | Where-Object { $keep -notcontains $_.Name } | Remove-Item -Recurse -Force
$global:LASTEXITCODE = 0

# 4. Optional zip (tar ships with Windows 10 and later, and handles large archives).
if ($Zip) {
    $zipPath = Join-Path $release 'Card Conjurer.zip'
    if (Test-Path $zipPath) { Remove-Item $zipPath -Force }
    Write-Host 'Zipping...' -ForegroundColor Cyan
    Push-Location $release
    try {
        tar -a -c -f 'Card Conjurer.zip' 'Card Conjurer'
        if ($LASTEXITCODE -ne 0) { throw "tar failed with exit code $LASTEXITCODE" }
    } finally {
        Pop-Location
    }
}

$sizeGB = (Get-ChildItem $appDir -Recurse -File | Measure-Object Length -Sum).Sum / 1GB
Write-Host ("Done: {0} ({1:N2} GB)" -f $appDir, $sizeGB) -ForegroundColor Green
if ($Zip) { Write-Host "Zip:  $(Join-Path $release 'Card Conjurer.zip')" -ForegroundColor Green }
