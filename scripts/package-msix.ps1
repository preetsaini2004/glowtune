# Builds the Microsoft Store package (MSIX) for Glowtune.
#
#   ./scripts/package-msix.ps1 -IdentityName <Package/Identity/Name> `
#       -Publisher "<Package/Identity/Publisher, CN=...>" `
#       -PublisherDisplayName "<Package/Properties/PublisherDisplayName>"
#
# The three values come from Partner Center → the app → Product identity.
# The package is unsigned on purpose: the Microsoft Store signs it after
# certification. Output: target/msix/Glowtune_<version>_x64.msix
#
# Add -SkipBuild to reuse target/release/glowtune.exe.
param(
    [Parameter(Mandatory)] [string] $IdentityName,
    [Parameter(Mandatory)] [string] $Publisher,
    [Parameter(Mandatory)] [string] $PublisherDisplayName,
    # "Edge" was rejected by Store certification (10.1.1.1: names another product).
    [string] $DisplayName = "Glowtune: Ambient Glow & Lyrics",
    [switch] $SkipBuild
)
$ErrorActionPreference = "Stop"
$root = Split-Path $PSScriptRoot -Parent

# Store versions have four parts and the last one must be 0: 0.1.0 → 0.1.0.0.
$conf = Get-Content "$root/app/src-tauri/tauri.conf.json" -Raw | ConvertFrom-Json
$version = "$($conf.version).0"

$sdk = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin\10.*\x64\makeappx.exe" |
    Sort-Object FullName | Select-Object -Last 1
if (-not $sdk) { throw "makeappx.exe not found: install the Windows 10/11 SDK." }
$makeappx = $sdk.FullName

if (-not $SkipBuild) {
    Push-Location "$root/app"
    # Build tools write progress to stderr; Windows PowerShell 5.1 would treat
    # that as an error, so judge the build by its exit code only.
    $ErrorActionPreference = "Continue"
    try { & cmd /c "pnpm tauri build --no-bundle 2>&1"; if ($LASTEXITCODE) { throw "build failed" } }
    finally { Pop-Location; $ErrorActionPreference = "Stop" }
}
$exe = "$root/target/release/glowtune.exe"
if (-not (Test-Path $exe)) { throw "$exe is missing; run without -SkipBuild." }

# Stage the package contents.
$stage = "$root/target/msix/stage"
Remove-Item $stage -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force "$stage/Assets" | Out-Null
Copy-Item $exe "$stage/glowtune.exe"
Copy-Item "$root/LICENSE" "$stage/LICENSE.txt"
$icons = "$root/app/src-tauri/icons"
Copy-Item "$icons/Square150x150Logo.png" "$stage/Assets/Square150x150Logo.png"
Copy-Item "$icons/Square44x44Logo.png"   "$stage/Assets/Square44x44Logo.png"
Copy-Item "$icons/StoreLogo.png"         "$stage/Assets/StoreLogo.png"

$esc = { param($s) [System.Security.SecurityElement]::Escape($s) }
$manifest = @"
<?xml version="1.0" encoding="utf-8"?>
<Package
  xmlns="http://schemas.microsoft.com/appx/manifest/foundation/windows10"
  xmlns:uap="http://schemas.microsoft.com/appx/manifest/uap/windows10"
  xmlns:rescap="http://schemas.microsoft.com/appx/manifest/foundation/windows10/restrictedcapabilities"
  IgnorableNamespaces="uap rescap">
  <Identity Name="$(& $esc $IdentityName)" Publisher="$(& $esc $Publisher)" Version="$version" ProcessorArchitecture="x64" />
  <Properties>
    <DisplayName>$(& $esc $DisplayName)</DisplayName>
    <PublisherDisplayName>$(& $esc $PublisherDisplayName)</PublisherDisplayName>
    <Logo>Assets\StoreLogo.png</Logo>
    <Description>Ambient edge glow, synced lyrics and a living desktop scene for your music.</Description>
  </Properties>
  <Dependencies>
    <TargetDeviceFamily Name="Windows.Desktop" MinVersion="10.0.17763.0" MaxVersionTested="10.0.26100.0" />
  </Dependencies>
  <Resources>
    <Resource Language="en-us" />
  </Resources>
  <Applications>
    <Application Id="Glowtune" Executable="glowtune.exe" EntryPoint="Windows.FullTrustApplication">
      <uap:VisualElements
        DisplayName="Glowtune"
        Description="Ambient edge glow, synced lyrics and a living desktop scene for your music."
        BackgroundColor="transparent"
        Square150x150Logo="Assets\Square150x150Logo.png"
        Square44x44Logo="Assets\Square44x44Logo.png">
      </uap:VisualElements>
    </Application>
  </Applications>
  <Capabilities>
    <Capability Name="internetClient" />
    <rescap:Capability Name="runFullTrust" />
  </Capabilities>
</Package>
"@
[System.IO.File]::WriteAllText("$stage/AppxManifest.xml", $manifest, (New-Object System.Text.UTF8Encoding $false))

$out = "$root/target/msix/Glowtune_${version}_x64.msix"
Remove-Item $out -ErrorAction SilentlyContinue
& $makeappx pack /o /d $stage /p $out
if ($LASTEXITCODE) { throw "makeappx failed" }
Write-Host "`nStore package: $out"
