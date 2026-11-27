# NVIDIA memory and VRAM A/B benchmark

Use this procedure to measure the memory-path changes on the same title and
scene on the RTX 5050 Laptop 8 GB system. The implementation keeps full-refresh
and scratch-buffer fallbacks. No FPS or bandwidth improvement is claimed
until the target laptop run confirms it.

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
- shader-written texture readbacks landed directly into title memory versus
  using the scratch fallback.

Also record shader-write copy-back scan and sync time, bytes compared,
host private commit/working set, texture staging bytes, dedicated/local VRAM
usage and budget, CPU frame/submit time, GPU busy time, and 95th-percentile
frame time. Note visible mip/streaming/rendering artifacts.

## What the changes should affect

The dirty 4 KiB write-watch masks narrow CPU texture reads and detiles to
affected mip/slice ranges. On tracked physical-page views, the same masks
help when the guard observed the write; periodic full hashes remain for alias
writes that the guard cannot see. The GPU upload records copy only selected
subresources and merge repeated selections before the safe deferred refresh.

Fully committed shader-written texture readbacks can land directly into title
memory after layout/footprint bounds are preflighted. Partially mapped or
uncertain allocations use the original scratch/read/copy-back path.

Shader-written buffer copy-back reuses old-title scratch storage across pieces
and uses a 32-byte AVX2 three-way merge when supported, with a scalar fallback.
The title's concurrent writes still win over stale GPU bytes.

## Interpretation

Compare medians across the three measured passes, not the best run. Lower
subresource-copy counts, more in-place readbacks, or lower CPU copy-back time
are useful only when accompanied by lower memory traffic and frame-time
percentiles without image corruption, late mip streaming or device loss.
