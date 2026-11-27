param(
    [Parameter(Mandatory)][string]$Exe,
    [ValidateRange(0, 4)][int]$DropMips = 0,
    [string]$Resolution = '1920x1080',
    [ValidateSet('series-s', 'series-x')]
    [string]$Console = $(if ([string]::IsNullOrWhiteSpace($env:VICTORY_CONSOLE)) {
        'series-x'
    } else {
        $env:VICTORY_CONSOLE.ToLowerInvariant()
    }),
    # Optional cap on eligible tiled 2D title-memory texture uploads. 0 disables it.
    [ValidateRange(0, 16384)][int]$TextureMaxDim = 0,
    [switch]$Borderless,
    [switch]$Water,
    [switch]$NoOverlay,
    # Lower idle pool and window sizes for low-memory runs (see docs/NVIDIA-INTEGRATION.md).
    [switch]$LowMemory,
    # Free physical memory in MiB under which idle memory is trimmed (0: off).
    [ValidateRange(0, 262144)][int]$RamTrimMB = 0,
    # Log a memory census every N frames (0: off).
    [ValidateRange(0, 100000)][int]$CensusFrames = 0,
    # Disable all telemetry for this run.
    [switch]$NoTelemetry
)

$repoRoot = Split-Path -Parent $PSScriptRoot
$runner = Join-Path $repoRoot 'target\release\victory.exe'
$launcher = Join-Path $repoRoot 'Play-VIctory.cmd'

if (-not (Test-Path -LiteralPath $runner -PathType Leaf)) {
    throw "Release executable missing at $runner. Run cargo build --release --locked -p victory."
}
if (-not (Test-Path -LiteralPath $Exe -PathType Leaf)) {
    throw "Title executable missing at $Exe."
}
if (-not (Test-Path -LiteralPath $launcher -PathType Leaf)) {
    throw "Repository launcher missing at $launcher."
}

$settings = @{
    VICTORY_TITLE_EXE = (Resolve-Path -LiteralPath $Exe).ProviderPath
    VICTORY_RUNNER = $runner
    VICTORY_CONSOLE = $Console
    VICTORY_RESOLUTION = $Resolution
    VICTORY_TEXTURE_DROP_MIPS = [string]$DropMips
}
# Preserve the caller's environment unless this option was explicitly supplied.
if ($PSBoundParameters.ContainsKey('TextureMaxDim')) {
    $settings.VICTORY_TEXTURE_MAX_DIM = [string]$TextureMaxDim
}
if ($Borderless) { $settings.VICTORY_BORDERLESS = '1' }
if ($Water) { $settings.VICTORY_TEXTURE_BACK = '1' }
if ($LowMemory) {
    $lowMemory = @{
        VICTORY_WINDOW_KB = '512'
        VICTORY_WINDOW_TAILS = '1'
        VICTORY_STAGING_POOL_MB = '64'
        VICTORY_SCRATCH_SPARE_MB = '64'
        VICTORY_SNAPSHOT_POOL_MB = '128'
        VICTORY_TEXTURE_POOL_SPARE_MB = '256'
        VICTORY_VIEW_COPIES_MB = '32'
    }
    foreach ($key in $lowMemory.Keys) {
        if (-not (Test-Path "Env:$key")) { $settings[$key] = $lowMemory[$key] }
    }
}
if ($RamTrimMB -gt 0) { $settings.VICTORY_RAM_TRIM_MB = [string]$RamTrimMB }
if ($CensusFrames -gt 0) { $settings.VICTORY_CENSUS_FRAMES = [string]$CensusFrames }
if ($NoTelemetry) { $settings.VICTORY_TELEMETRY = '0' }
if ($NoOverlay) {
    $settings.VICTORY_OVERLAY = '0'
    $settings.VICTORY_PANEL = '0'
} elseif (-not (Test-Path Env:VICTORY_OVERLAY)) {
    $settings.VICTORY_OVERLAY = '1'
}

# Change only settings owned by this launcher, then put the caller's values back.
$saved = @{}
try {
    foreach ($key in $settings.Keys) {
        $saved[$key] = [Environment]::GetEnvironmentVariable($key, 'Process')
        [Environment]::SetEnvironmentVariable($key, $settings[$key], 'Process')
    }
    & $launcher pareto-residency
    $exitCode = $LASTEXITCODE
} finally {
    foreach ($key in $saved.Keys) {
        [Environment]::SetEnvironmentVariable($key, $saved[$key], 'Process')
    }
}
if ($null -ne $exitCode) {
    $global:LASTEXITCODE = $exitCode
}
