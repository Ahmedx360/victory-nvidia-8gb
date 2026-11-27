param(
    [Parameter(Mandatory)][string]$Exe,
    [ValidateSet('series-s', 'series-x')]
    [string]$Console = $(if ([string]::IsNullOrWhiteSpace($env:VICTORY_CONSOLE)) {
        'series-x'
    } else {
        $env:VICTORY_CONSOLE.ToLowerInvariant()
    }),
    [string]$Resolution = '1920x1080',
    # This is an opt-in quality/performance trade-off for eligible tiled 2D textures.
    [ValidateRange(0, 16384)][int]$TextureMaxDim = 1024,
    [ValidateRange(0, 4)][int]$DropMips = 0,
    [switch]$Borderless,
    [switch]$Water,
    [switch]$NoOverlay,
    # The 8 GB preset reduces retained idle allocations; use this to keep stock pool sizes.
    [switch]$NoLowMemory,
    [switch]$WaitForTextureMemory,
    # Troubleshooting only: revert the MONITORX/MWAITX code patch.
    [switch]$DisableWaitPatching,
    # Kept so older invocations do not fail; this repository has no MWAITX sleep mode.
    [switch]$NoIntelFenceSleep,
    [ValidateRange(0, 262144)][int]$RamTrimMB = 0,
    [ValidateRange(0, 100000)][int]$CensusFrames = 0,
    [switch]$NoTelemetry
)

$repoRoot = Split-Path -Parent $PSScriptRoot
$launchScript = Join-Path $PSScriptRoot 'play-nvidia-8gb.ps1'
if (-not (Test-Path -LiteralPath $launchScript -PathType Leaf)) {
    throw "NVIDIA profile launcher missing at $launchScript."
}
if (-not (Test-Path -LiteralPath $Exe -PathType Leaf)) {
    throw "Title executable missing at $Exe."
}

if ($NoIntelFenceSleep) {
    Write-Warning 'NoIntelFenceSleep is obsolete: VICTORY_MWAITX_SLEEP is not used by this branch. No wait mode is changed. Use -DisableWaitPatching only for a troubleshooting comparison.'
}
if ($DropMips -ne 0) {
    Write-Warning 'DropMips is retained for older launch scripts. The integration guide marks forced mip dropping as experimental; prefer -TextureMaxDim for the supported low-VRAM experiment.'
}

$overrides = @{}
$temporary = @{}
if ($WaitForTextureMemory) {
    $temporary.VICTORY_TEXTURE_NO_WAIT = '0'
}
if ($DisableWaitPatching) {
    $temporary.VICTORY_PATCH_WAITS = '0'
}
$launchArgs = @{
    Exe = $Exe
    Console = $Console
    Resolution = $Resolution
    TextureMaxDim = $TextureMaxDim
    DropMips = $DropMips
    RamTrimMB = $RamTrimMB
    CensusFrames = $CensusFrames
}
if (-not $NoLowMemory) { $launchArgs.LowMemory = $true }
if ($Borderless) { $launchArgs.Borderless = $true }
if ($Water) { $launchArgs.Water = $true }
if ($NoOverlay) { $launchArgs.NoOverlay = $true }
if ($NoTelemetry) { $launchArgs.NoTelemetry = $true }

Write-Host 'VIctory NVIDIA 8 GB profile'
Write-Host "  Console: $Console"
Write-Host "  Resolution cap: $Resolution"
Write-Host "  Texture max dimension: $TextureMaxDim (0 disables the cap)"
Write-Host "  Reduced idle-memory retention: $(-not $NoLowMemory)"
Write-Host "  Non-blocking texture refresh: $(-not $WaitForTextureMemory)"
Write-Host "  Overlay: $(-not $NoOverlay)"
Write-Host '  Use the same save, scene and camera position for A/B comparisons.'

$exitCode = $null
try {
    foreach ($key in $temporary.Keys) {
        $overrides[$key] = [Environment]::GetEnvironmentVariable($key, 'Process')
        [Environment]::SetEnvironmentVariable($key, $temporary[$key], 'Process')
    }
    & $launchScript @launchArgs
    $exitCode = $LASTEXITCODE
} finally {
    foreach ($key in $overrides.Keys) {
        [Environment]::SetEnvironmentVariable($key, $overrides[$key], 'Process')
    }
}
if ($null -ne $exitCode) {
    $global:LASTEXITCODE = $exitCode
}
