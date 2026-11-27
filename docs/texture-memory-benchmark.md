# NVIDIA texture-memory A/B benchmark

This procedure validates dirty-subresource texture refresh, guarded
physical-page refresh and selective D3D12 texture copies. Conservative
full-copy fallbacks remain enabled. Do not compare different scenes or use
the first shader-cache run as the performance sample.

## Builds

Comparison pair:

- Baseline: `71a373b69611cf5bf193bdf8d261f78190cfb752`
- Optimized: record the exact `main` commit SHA.

Use two worktrees and keep title files outside both:

```powershell
git fetch origin
git worktree add ..\victory-memory-baseline 71a373b69611cf5bf193bdf8d261f78190cfb752
git worktree add ..\victory-memory-optimized origin/main
```

Build each with the pinned toolchain:

```powershell
cargo build --release --locked -p victory
```

Keep the NVIDIA driver, console profile, 1920x1080 resolution, title
executable, save, camera path and title settings identical. Warm shaders and
pipelines before capturing measured runs. Use separate cache directories.

## Launch

Use identical settings for both worktrees:

```powershell
.\scripts\play-nvidia-8gb-optimized.ps1 -Exe 'D:\path\to\your\title.exe' -CensusFrames 120 -NoOverlay -NoTelemetry
```

Keep Series X and the same 1024 texture cap in both runs. Do not use
`-WaitForTextureMemory` or `-DisableWaitPatching`, and do not change
`VICTORY_TEXTURE_READ_CACHE_MB` between runs. Repeat both builds with
`-TextureMaxDim 0` for an independent texture-cap comparison.

## Capture

Run the same scene for at least three measured passes per build after warm-up.

| Metric | Source |
|---|---|
| Median and 95th-percentile frame time | Frame diagnostics, same interval |
| CPU submit time and GPU busy time | Per-frame diagnostics |
| Private commit and working set | `memory census` |
| Texture staging bytes and pool retention | `memory census` |
| Dedicated/local VRAM used and budget | DXGI memory census |
| Texture bytes refreshed | Texture/copy diagnostics |
| Texture subresources copied per frame | Texture-upload records |
| Streaming/mip/rendering artifacts | Same camera path and screenshots |

Compare medians across the three passes, not the best run. Record logs and
note visual mip, lighting or streaming regressions. A gain is not established
until copy traffic or stalls improve without visual errors or device loss.

## Interpretation

The expected mechanism is fewer title-memory reads, fewer CPU detiles and
fewer GPU subresource copies when a small part of a texture changes. Highly
fragmented or near-whole updates, packed tails, on-demand residency, partial
mappings and uncertain layouts may still use full refreshes. The periodic
whole-texture hash remains to detect writes through aliases that the guard
cannot observe. Lower CPU copy traffic does not automatically mean lower GPU
frame time; measure those costs separately.
