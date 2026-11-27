# NVIDIA texture-memory A/B benchmark

Use this to determine whether the texture-memory work improves the same title
and scene on the RTX 5050 Laptop 8 GB system. The implementation keeps full
refresh fallbacks; no FPS or bandwidth gain is assumed until a local GPU run
confirms it.

## Comparison builds

- Baseline: `71a373b69611cf5bf193bdf8d261f78190cfb752`
- Optimized: record the exact current `main` SHA with the log.

Build two worktrees with the same pinned Rust toolchain:

```powershell
git fetch origin
git worktree add ..\victory-memory-baseline 71a373b69611cf5bf193bdf8d261f78190cfb752
git worktree add ..\victory-memory-optimized origin/main
cd ..\victory-memory-baseline
cargo build --release --locked -p victory
cd ..\victory-memory-optimized
cargo build --release --locked -p victory
```

Use the same title executable, driver, console model, save, 1920x1080 output,
settings, camera path and shader-cache warm-up for both builds. Keep their
cache directories separate.

## Capture

For each build, capture at least three passes of the same 60–120 second scene
after one warm-up pass. Enable diagnostics and a periodic memory census using
`VICTORY_CENSUS_FRAMES=120`. Do not change texture cap,
`VICTORY_TEXTURE_READ_CACHE_MB`, memory budgets or wait switches between
the pair.

Record the renderer's texture-upload summary:

- texture upload commands recorded;
- D3D12 texture subresources actually copied;
- subresources omitted by partial copies;
- subresources omitted by whole-upload skips;
- whole texture uploads skipped as unchanged;
- shader-written texture readbacks landed in place versus scratch-backed.

Also record CPU frame/submit time, GPU busy time, texture bytes refreshed,
texture staging bytes, host private commit/working set, dedicated/local VRAM
usage and budget, plus any visible mip/streaming/rendering artifacts.

## How to interpret it

The expected win is lower CPU memory traffic and fewer D3D12 texture-copy
regions when a small subset of a texture changes. The 4 KiB write-watch masks
drive the selective path for tracked title memory and for physical-page views
when their guard observed the write. Periodic whole-texture verification stays
enabled for alias writes the guard cannot see.

On-demand residency changes, partially mapped textures, unknown layouts,
highly fragmented ranges and changes covering at least 75% of the texture
retain full refreshes. The direct texture readback landing path is enabled
only when the title allocation is fully committed and every tiling bound is
preflighted; all other cases retain the old scratch/read/copy-back path.

Compare medians across the three measured passes, not the best run. A lower
subresource-copy count alone is not proof of a faster frame: memory-census
figures and frame-time percentiles must improve without image corruption,
late mip streaming or device loss.
