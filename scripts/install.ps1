# Install the lgtm CLI into %USERPROFILE%\.lgtm\bin.
#
#   powershell -c "irm lgtm.arsenstorm.com/install.ps1 | iex"
#
# $env:LGTM_VERSION="v0.2.0" pins a tag; $env:LGTM_RELEASE_BASE points at
# another host.
$ErrorActionPreference = "Stop"
# Windows PowerShell 5.1 can still default to TLS 1.0, which GitHub rejects.
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12

$Base = if ($env:LGTM_RELEASE_BASE) { $env:LGTM_RELEASE_BASE } else { "https://github.com/arsenstorm/lgtm/releases" }
$Version = if ($env:LGTM_VERSION) { $env:LGTM_VERSION } else { "latest" }
$Prefix = Join-Path $env:USERPROFILE ".lgtm\bin"

if ($env:PROCESSOR_ARCHITECTURE -eq "ARM64") {
    Write-Error "unsupported platform: Windows ARM64 (no lgtm build yet)"
    exit 1
}
$Target = "x86_64-pc-windows-msvc"

$Url = if ($Version -eq "latest") { "$Base/latest/download" } else { "$Base/download/$Version" }
$File = "lgtm-$Target.zip"

$Tmp = Join-Path ([IO.Path]::GetTempPath()) ("lgtm-" + [Guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Path $Tmp | Out-Null
try {
    $Zip = Join-Path $Tmp $File
    $Sums = Join-Path $Tmp "SHA256SUMS"
    Invoke-WebRequest -UseBasicParsing -Uri "$Url/$File" -OutFile $Zip
    Invoke-WebRequest -UseBasicParsing -Uri "$Url/SHA256SUMS" -OutFile $Sums

    $Line = Get-Content $Sums | Where-Object { $_ -match "\s$([Regex]::Escape($File))$" } | Select-Object -First 1
    if (-not $Line) { throw "no checksum for $File in SHA256SUMS" }
    $Expected = ($Line -split "\s+")[0]
    $Actual = (Get-FileHash -Algorithm SHA256 $Zip).Hash
    if ($Actual -ne $Expected.ToUpper()) { throw "checksum mismatch for $File" }

    New-Item -ItemType Directory -Force -Path $Prefix | Out-Null
    Expand-Archive -Path $Zip -DestinationPath $Prefix -Force
}
finally {
    Remove-Item -Recurse -Force $Tmp -ErrorAction SilentlyContinue
}

$PathAdded = $false
$UserPath = [Environment]::GetEnvironmentVariable("Path", "User")
if (($UserPath -split ";") -notcontains $Prefix) {
    $Joined = if ($UserPath) { "$UserPath;$Prefix" } else { $Prefix }
    [Environment]::SetEnvironmentVariable("Path", $Joined, "User")
    $env:Path = "$env:Path;$Prefix"
    $PathAdded = $true
}

# Prints "lgtm <version>" once the CLI takes --version; plain "lgtm" until then.
$Reported = try { & (Join-Path $Prefix "lgtm.exe") --version 2>$null } catch { "lgtm" }
if (-not $Reported) { $Reported = "lgtm" }
Write-Output "installed $Reported to ~\.lgtm\bin\lgtm.exe"
if ($PathAdded) {
    Write-Output "open a new shell or run: `$env:Path = `"`$env:Path;$Prefix`""
}
Write-Output "next: lgtm serve"
