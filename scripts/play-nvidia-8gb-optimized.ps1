param(
    [Parameter(Mandatory = $true)]
    [string]$Exe,
    [int]$DropMips = 2,
    [string]$Resolution = "1920x1080",
    [switch]$Borderless,
    [switch]$Water,
    [switch]$NoOverlay,
    [switch]$WaitForTextureMemory,
    [switch]$NoIntelFenceSleep
)

$repoRoot = Split-Path -Parent $PSScriptRoot
$victory = Join-Path $repoRoot "target\release\victory.exe"

if (-not (Test-Path $victory)) {
    Write-Error "Release binary not found at $victory. Build it first with: cargo build --release --bin victory"
    exit 1
}

# Start from a clean environment so stale VICTORY_* settings cannot override
# the profile below.
Remove-Item Env:VICTORY_* -ErrorAction SilentlyContinue

if ($Resolution) {
    $env:VICTORY_RESOLUTION = $Resolution
}
if ($Borderless) {
    $env:VICTORY_BORDERLESS = "1"
}
if ($Water) {
    $env:VICTORY_TEXTURE_BACK = "1"
}
if ($NoOverlay) {
    $env:VICTORY_OVERLAY = "0"
}

$env:VICTORY_TEXTURE_DROP_MIPS = "$DropMips"
$env:VICTORY_COPY_PLAIN = "1"
$env:VICTORY_INDIRECT_GATHER = "1"
$env:VICTORY_HEAP_WORKER = "1"
$env:VICTORY_ALIAS_ONCE = "1"
$env:VICTORY_JOIN_PARTS = "1"
$env:VICTORY_BACK_OWN_VIEW = "1"
$env:VICTORY_INDIRECT_WINDOW_DRAWS = "1"
$env:VICTORY_GUARD_HOLES_FRAME = "1"
$env:VICTORY_COPY_CHANGED = "1"
$env:VICTORY_CARD_WINDOWS_MB = "2048"
$env:VICTORY_CARD_BUFFERS_MB = "1024"
$env:VICTORY_CLAMP_WAITS = "0"
$env:VICTORY_WIDE_MERGE = "1"
$env:VICTORY_RELAY_OWN_QUEUE = "1"

# Main fix for the long texture stalls in the supplied diagnostics:
# do not block texture refresh while the title runtime remaps its memory.
# A texture may appear one frame later, but is never read partially.
if ($WaitForTextureMemory) {
    $env:VICTORY_TEXTURE_NO_WAIT = "0"
} else {
    $env:VICTORY_TEXTURE_NO_WAIT = "1"
}

# Intel hybrid CPU optimization: let long emulated fence waits sleep and
# release a hardware thread for the submit/recording work.
if (-not $NoIntelFenceSleep) {
    $env:VICTORY_MWAITX_SLEEP = "1"
}

Write-Host "VIctory NVIDIA profile"
Write-Host "  Resolution: $Resolution"
Write-Host "  Texture mip drop: $DropMips"
Write-Host "  Non-blocking texture refresh: $(-not $WaitForTextureMemory)"
Write-Host "  Intel fence-wait sleep: $(-not $NoIntelFenceSleep)"
Write-Host "  Card windows/buffers budgets: 2048 MB / 1024 MB"
Write-Host "  Compare runs in the same scene and camera position."

& $victory run --execute $Exe
