# Packs veilq.exe into the MSIX package for the Microsoft Store (ADR-0039):
# fills in packaging/msix/AppxManifest.xml and runs the Windows SDK's
# makeappx. The package is unsigned; the Store signs it.
#
#   pwsh scripts/msix.ps1 -Exe target/release/veilq.exe -Out dist/veilq.msix
#
# Without -Name/-Publisher/-PublisherName it uses test values: such a
# package is for a check of the layout (or `Add-AppxPackage -Register` of
# the layout folder in developer mode), not for the Store. The values for the
# Store come from Partner Center (Product identity).
param(
    [Parameter(Mandatory)] [string] $Exe,
    [Parameter(Mandatory)] [string] $Out,
    [string] $Version,
    [string] $Name = 'veilq.test',
    [string] $Publisher = 'CN=veilq-test',
    [string] $PublisherName = 'veilq (test)',
    # Where the filled-in layout is kept; a temporary folder by default.
    [string] $Layout
)
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent

if (-not $Version) {
    $line = Select-String -Path (Join-Path $root 'Cargo.toml') -Pattern '^version = "(.+)"$' | Select-Object -First 1
    $Version = $line.Matches[0].Groups[1].Value
}
# The Store's version has four parts, and it reserves the fourth: 0.
if ($Version -notmatch '^\d+\.\d+\.\d+$') { throw "version '$Version' is not major.minor.patch" }
$Version = "$Version.0"

$makeappx = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin\*\x64\makeappx.exe" -ErrorAction SilentlyContinue |
    Sort-Object { [version]$_.Directory.Parent.Name } | Select-Object -Last 1
if (-not $makeappx) { throw 'makeappx.exe not found: install the Windows SDK' }

if (-not $Layout) { $Layout = Join-Path ([IO.Path]::GetTempPath()) "veilq-msix-$PID" }
if (Test-Path $Layout) { Remove-Item -Recurse -Force $Layout }
New-Item -ItemType Directory -Force (Join-Path $Layout 'Assets') | Out-Null
Copy-Item (Join-Path $root 'packaging/msix/Assets/*.png') (Join-Path $Layout 'Assets')
Copy-Item $Exe (Join-Path $Layout 'veilq.exe')

$esc = { param($s) [Security.SecurityElement]::Escape($s) }
$manifest = (Get-Content (Join-Path $root 'packaging/msix/AppxManifest.xml') -Raw).
    Replace('@NAME@', (& $esc $Name)).
    Replace('@PUBLISHER@', (& $esc $Publisher)).
    Replace('@PUBLISHER_NAME@', (& $esc $PublisherName)).
    Replace('@VERSION@', $Version)
if ($manifest -match '@[A-Z_]+@') { throw "a value in the manifest is not filled in: $($Matches[0])" }
Set-Content -Path (Join-Path $Layout 'AppxManifest.xml') -Value $manifest -NoNewline -Encoding utf8

$outDir = Split-Path $Out -Parent
if ($outDir) { New-Item -ItemType Directory -Force $outDir | Out-Null }
& $makeappx.FullName pack /d $Layout /p $Out /o
if ($LASTEXITCODE -ne 0) { throw "makeappx failed ($LASTEXITCODE)" }
"packed $Out ($Name $Version, $($makeappx.Directory.Parent.Name))"
