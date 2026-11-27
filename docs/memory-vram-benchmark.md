# NVIDIA memory and VRAM A/B benchmark

Use this procedure to measure the memory-path changes on the same title and
scene on the RTX 5050 Laptop 8 GB system. Full-refresh and scratch-buffer
fallbacks remain enabled; no FPS or bandwidth gain is assumed until a local
GPU run confirms it.

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
after one warm-up pass. Enable diagnostics and a periodic memory census with
`VICTORY_CENSUS_FRAMES=120`. Do not change texture cap,
`VICTORY_TEXTURE_READ_CACHE_MB`, memory budgets or wait switches between
the pair.

Record the diagnostics:

- texture upload commands recorded;
- D3D12 texture subresources actually copied;
- subresources omitted by partial copies or whole-upload skips;
- shader-written texture readbacks landed directly into title memory versus
  using the scratch fallback;
- shadow-buffer refresh bytes read directly versus through the kernel-copy
  path;
- shader-written buffer pieces that differed from the snapshot;
- shader-written pieces merged in parallel;
- total shader copy-back sync time.

Also record CPU frame/submit time, GPU busy time, texture bytes refreshed,
texture staging bytes, host private commit/working set, dedicated/local VRAM
usage and budget, and any visible mip/streaming/rendering artifacts.

## What the changes should affect

The dirty 4 KiB write-watch masks narrow CPU texture reads and detiles to
affected mip/slice ranges. On tracked physical-page views, the same masks help
when the guard observed the write; periodic full hashes remain for alias writes
the guard cannot see. GPU upload commands copy only selected subresources and
merge repeated selections before the safe deferred refresh.

Fully committed shader-written texture readbacks can land directly into title
memory after layout/footprint bounds are preflighted. Partial or uncertain
allocations use the original scratch/read/copy-back path.

Large title-buffer shadow refreshes try direct reads in bounded 4 MiB chunks.
A chunk is read in place only while the mapping lock is acquired without
waiting and its pages are all readable; otherwise the guarded kernel-copy
routine handles that chunk. Card verification retains its existing kernel
copy sequence.

Shader-written buffer copy-back reuses per-thread scratch and uses a 32-byte
AVX2 three-way merge on supported CPUs. Eight or more ordered, disjoint 64 KiB
pieces are merged on the persistent worker pool; smaller or uncertain lists
stay serial. The snapshot lock is held until all worker jobs return, and the
guarded conditional writer preserves concurrent title updates.

## Interpretation

Compare medians across the three measured passes, not the best run. Lower
subresource-copy counts, more in-place readbacks, more direct shadow-read
bytes, or lower copy-back time are useful only when accompanied by lower
memory traffic and frame-time percentiles without image corruption, late mip
streaming or device loss.
