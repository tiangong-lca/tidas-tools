[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$Version,

    [string]$Destination = "$env:LOCALAPPDATA\Programs\tidas\bin",

    [string]$Repository = "tiangong-lca/tidas-tools"
)

$ErrorActionPreference = "Stop"
$Version = $Version.TrimStart("v")
if ([string]::IsNullOrWhiteSpace($Version)) {
    throw "Version must be an explicit immutable release version."
}

if ([System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture -ne "X64") {
    throw "The initial release supports Windows x86_64 only. Windows ARM64 is tracked separately."
}

$Target = "x86_64-pc-windows-msvc"
$Archive = "tidas-v$Version-$Target.zip"
$BaseUrl = "https://github.com/$Repository/releases/download/v$Version"
$Temporary = Join-Path ([System.IO.Path]::GetTempPath()) ("tidas-install-" + [guid]::NewGuid())

try {
    New-Item -ItemType Directory -Path $Temporary | Out-Null
    $ArchivePath = Join-Path $Temporary $Archive
    $ChecksumPath = "$ArchivePath.sha256"
    Invoke-WebRequest -UseBasicParsing -Uri "$BaseUrl/$Archive" -OutFile $ArchivePath
    Invoke-WebRequest -UseBasicParsing -Uri "$BaseUrl/$Archive.sha256" -OutFile $ChecksumPath

    $ChecksumLine = (Get-Content -Raw $ChecksumPath).Trim()
    $Parts = $ChecksumLine -split "\s+", 2
    if ($Parts.Count -ne 2 -or $Parts[1] -ne $Archive) {
        throw "Checksum file does not name $Archive."
    }
    $Actual = (Get-FileHash -Algorithm SHA256 $ArchivePath).Hash.ToLowerInvariant()
    if ($Actual -ne $Parts[0].ToLowerInvariant()) {
        throw "SHA-256 mismatch for $Archive."
    }

    $Extracted = Join-Path $Temporary "extracted"
    Expand-Archive -LiteralPath $ArchivePath -DestinationPath $Extracted
    $Source = Join-Path $Extracted "tidas-v$Version-$Target\bin\tidas.exe"
    if (-not (Test-Path -LiteralPath $Source -PathType Leaf)) {
        throw "Verified archive does not contain bin\tidas.exe."
    }

    New-Item -ItemType Directory -Force -Path $Destination | Out-Null
    $Installed = Join-Path $Destination "tidas.exe"
    Copy-Item -LiteralPath $Source -Destination $Installed -Force
    & $Installed --version
    Write-Output "Installed verified tidas v$Version to $Installed"
    Write-Output "Add $Destination to PATH if it is not already present."
}
finally {
    if (Test-Path -LiteralPath $Temporary) {
        Remove-Item -LiteralPath $Temporary -Recurse -Force
    }
}
