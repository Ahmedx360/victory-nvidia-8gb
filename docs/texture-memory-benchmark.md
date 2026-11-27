# NVIDIA texture-memory A/B benchmark

This procedure is for validating the dirty-subresource refresh and selective
D3D12 texture-copy path. The code changes deliberately retain full-copy
fallbacks. **Do not compare different scenes or use the first shader-cache
run as the performance sample.**

## Builds

Use these two revisions as the comparison pair:

- Baseline before the texture-memory work: `71a373b69611cf5bf193bdf8d261f78190cfb752`
- Optimized working tree: `main` (record the exact commit SHA used)

Build each in a separate worktree and keep the title files outside both:

```powershell
git fetch origin
git worktree add ..\victory-memory-baseline 71a373b69611cf5bf193bdf8d261f78190cfb752
git worktree add ..\victory-memory-optimized origin/main
```

In each worktree, build with the pinned toolchain:

```powershell
cargo build --release --locked -p victory
```

Keep the same driver, console model, 1920x1080 resolution, game executable,
save, camera position, and title settings. Warm shaders and pipelines before
capturing the measured runs. Use separate cache directories for the two
builds.

## Launch

Use the same launcher options in both worktrees:

```powershell
.\scripts\play-nvidia-8gb-optimized.ps1 -Exe 'D:\path\to\your\title.exe' -CensusFrames 120 -NoOverlay -NoTelemetry
```

Keep the default Series X profile and the same 1024 texture cap in both runs.
Do not add `-WaitForTextureMemory`, `-DisableWaitPatching`, or change
`VICTORY_TEXTURE_READ_CACHE_MB` between measurements. For an independent
texture-cap comparison, repeat both builds with `-TextureMaxDim 0`.

## Capture and compare

Run the same scene for at least three measured passes per build after the
warm-up. Record:

| Metric | Where to get it |
|---|---|
| Median frame time and 95th percentile | Overlay or frame diagnostics, same sampled interval |
| CPU submit time and GPU busy time | Per-frame diagnostics |
| Private commit and working set | `memory census` lines |
| Texture staging bytes and pool retention | `memory census` lines |
| Dedicated/local VRAM used and budget | DXGI memory census |
| Texture bytes refreshed | `textures` / copy diagnostics |
| Texture subresources copied per frame | Diagnostics `texture upload` records; count unique subresources |
| Visible texture streaming and artifacts | Same camera path, captured screenshots |

Compare medians across the three passes, not the best run. Keep the log and
write down any visible mip, lighting, or streaming regressions. The code-path
changes are not considered an improvement until they reduce measured copy
traffic or stalls without introducing visual errors or device loss.

## Interpretation

The expected mechanism is narrower title-memory reads, fewer CPU detiles, and
fewer GPU subresource copies when only a small part of a texture changes.
Fully changed, highly fragmented, packed-tail/on-demand or uncertain cases may
still use the full path. A reduction in CPU copy bytes does not automatically
mean a lower GPU frame time; the measurements above separate those costs.
