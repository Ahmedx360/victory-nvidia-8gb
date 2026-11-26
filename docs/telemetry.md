# Anonymous run telemetry

`adel VIctory` sends one anonymous performance report for every
`victory run --execute`. It is **on by default** so frame-rate regressions can
be compared between builds and machines. Reports go to
`https://rxnd.de/vi/v1`; a complete local copy is written first, so a network
failure never loses a report and never affects the game.

## Opting out

Any one of these turns it off completely (no registration, no ping, no upload,
no recorder thread):

```powershell
$env:VICTORY_TELEMETRY = '0'          # environment, highest precedence
victory run ... --no-telemetry        # per-run CLI switch
```

```toml
# victory.toml
[telemetry]
enabled = false
```

`victory config` prints the resolved state (`telemetry : on|off`). The
environment variable takes precedence over `victory.toml`; `--no-telemetry`
takes precedence over both. `VICTORY_TELEMETRY=1` explicitly turns it back on.

## What is sent

The report identifies the **build**, not the player:

| Field | Source |
| --- | --- |
| `fork.name`, `fork.version`, `fork.git_sha` | `adel`, crate version, commit stamped at build time |
| `manifest.title.hash` | BLAKE3 of the original title executable |
| `manifest.title.console` | `Xbox Series X` or `Xbox Series S` |
| `manifest.os` | Windows product name and display version |
| `manifest.cpu` | Processor name and logical core count |
| `manifest.gpu` | Adapter name, driver version, dedicated VRAM |
| `manifest.ram_total_mb`, `manifest.display` | Total physical memory, primary screen width/height |

One sample is recorded for each one-second fps window the GPU present path
already measures (the same numbers shown in the title caption):

- `t_ms` — milliseconds since the recorder started;
- `fps`, `ms`, `draws` — frames/s, ms/frame, draws/frame;
- `cpu_ms`, `gpu_ms` — ms/frame spent preparing submissions and waiting on the
  GPU, from the existing submission counters;
- `cpu_pct`, `gpu_pct` — system CPU load and summed 3D-engine load, when the
  Windows counters are available;
- `ram_mb` — this process's working set;
- `cpu_temp_c` — CPU package temperature from Windows thermal zones, when that
  counter exists, plus `cpu_temp_reason` explaining `null` otherwise;
- `gpu_temp_c` — **not collected in v1**: `null` with reason
  `NVAPI temperature reader not enabled in v1`. GPU load and VRAM are present.

The client also computes mean/min/max fps, p50/p95/p99 frame time, 1% low,
longest stall and a stutter count. The server recomputes all of these from the
raw samples so a client bug cannot be mistaken for a real regression, and
renders an SVG FPS graph for each run.

**Never collected:** game files, save data, screenshots, window titles,
usernames, file paths, email addresses, IP or MAC addresses, or peripheral
serials. The server additionally redacts `C:\Users\<name>`, `/home/<name>`,
emails and IPv4 addresses as a backstop.

## Anonymous identity

On the first run the client derives a stable `machine_id`: the first 32 hex
digits of BLAKE3 over the CPU signature, the system volume serial and the
logical core count. No account name or path goes into it. It posts that ID to
`/v1/register` and stores the returned per-installation bearer token
(`vti_...`) in `<data_dir>/telemetry/token`. Every later ping and report uses
that token, so one PC's runs can be grouped without knowing who owns it. The
server stores only a hash of the token; the all-powerful admin token lives on
the server and is **never** compiled into or shipped with the client.

Optional: set `VICTORY_TELEMETRY_ALIAS` (for example `GTX1660-lab`) to label
this installation in the admin's user list. Keep it free of personal data.

## Local files

`<data_dir>/telemetry` (normally `%LOCALAPPDATA%\victory\data\telemetry`)
contains:

- `NOTICE.txt` — the first-run disclosure and opt-out instructions;
- `token` — this installation's bearer token;
- `manifest-<run_id>.json`, `samples-<run_id>.jsonl`, `summary-<run_id>.json`
  — one set per run, kept even when the upload fails.

## Server

Caddy terminates TLS on `rxnd.de/vi/*` and proxies to a loopback-only Python
service. SQLite is the API's source of truth (`installations`, `runs`,
`samples`, `events`, `heartbeats`); the same data is also kept as JSONL plus a
git archive of runs. There is currently no retention limit. Uploads use
WinHTTP through the Windows system trust store, with short connect and send
timeouts; a failed upload only writes a warning to the log.

Admin endpoints (require the server's admin token) include
`GET /vi/v1/users`, `GET /vi/v1/runs?git_sha=<sha>`, `GET /vi/v1/stats` and
`GET /vi/v1/runs/<id>/graph.svg`. Installation tokens can only read and write
their own runs. See the server README for deployment details.
