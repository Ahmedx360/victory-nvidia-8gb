//! Anonymous performance reports for adel VIctory.
//!
//! On by default, with `VICTORY_TELEMETRY=0` (or `[telemetry] enabled = false`,
//! or `victory run --no-telemetry`) as the opt-out. Every run writes a full
//! local bundle before attempting an HTTPS upload, so a network failure never
//! loses the data and never affects the game. Heavily compressed game-window
//! JPEGs are also captured every ten seconds for the first ten minutes, and
//! bounded Victory/title crash and game logs are uploaded at the end. No game
//! packages, usernames, or network addresses are collected; log text is
//! redacted on the server.

// On non-Windows hosts only the portable maths and tests are compiled: there
// is no recorder to call them, so the Windows-side plumbing is intentionally
// unused there.
#![cfg_attr(not(windows), allow(dead_code, unused_imports))]

pub mod json;
mod logs;
pub mod metrics;
mod summary;
#[cfg(windows)]
mod windows;

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use json::Json;
use summary::Sample;

/// Where reports are published. The only network destination in the crate.
pub const ENDPOINT: &str = "https://rxnd.de/vi/v1";

/// Everything the recorder needs that the app knows and the host does not.
#[derive(Clone, Debug)]
pub struct Options {
    /// `<data>/telemetry`: outside the repository, created by the caller.
    pub dir: PathBuf,
    /// BLAKE3 hash of the title executable the run mapped.
    pub title_hash: String,
    /// Console model the title was given.
    pub console: String,
    /// Explicit opt-out from the CLI or the config file.
    pub disabled: bool,
    /// Roots scanned at the end of the run for crash/game logs (for example
    /// `crashes.log` and the title `T:` temp directory).
    pub log_roots: Vec<PathBuf>,
}

/// Owns the recorder thread. `finish` joins it so the final bundle is written;
/// dropping it does the same, which covers early returns on the run path.
pub struct Run {
    stop: Option<Sender<()>>,
    thread: Option<JoinHandle<()>>,
}

impl Run {
    pub fn finish(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(thread) = self.thread.take()
            && thread.join().is_err()
        {
            tracing::warn!("telemetry recorder thread panicked");
        }
    }
}

impl Drop for Run {
    fn drop(&mut self) {
        self.finish();
    }
}

/// Starts one recorder for this process. Returns `None` when opted out or when
/// the recorder could not start; telemetry must never fail a game launch.
#[cfg(windows)]
pub fn start(options: Options) -> Option<Run> {
    if options.disabled || !enabled() {
        return None;
    }
    let (stop, rx) = mpsc::channel();
    let thread = std::thread::Builder::new()
        .name("victory-telemetry".into())
        .spawn(move || record(options, rx))
        .ok()?;
    Some(Run {
        stop: Some(stop),
        thread: Some(thread),
    })
}

/// Non-Windows builds have no host to measure: still compile the maths/tests,
/// but never start a recorder or contact a server.
#[cfg(not(windows))]
pub fn start(_options: Options) -> Option<Run> {
    None
}

fn enabled() -> bool {
    std::env::var_os("VICTORY_TELEMETRY").is_none_or(|v| v.to_str().is_none_or(|s| s != "0"))
}

fn unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = mp + if mp < 10 { 3 } else { -9 };
    (y + i64::from(m <= 2), m, d)
}

fn iso_utc() -> String {
    let secs = unix_seconds();
    let (y, m, d) = civil_from_days((secs / 86_400) as i64);
    let day = secs % 86_400;
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        day / 3600,
        (day % 3600) / 60,
        day % 60
    )
}

fn run_id() -> String {
    let stamp = iso_utc().replace(['-', ':', 'T', 'Z'], "");
    let salt = format!("{:?}:{}", SystemTime::now(), std::process::id());
    let suffix = &blake3::hash(salt.as_bytes()).to_hex()[..16];
    format!("{stamp}-{suffix}")
}

fn git_sha() -> &'static str {
    env!("VICTORY_TELEMETRY_GIT_SHA")
}

fn notice(dir: &std::path::Path) {
    let path = dir.join("NOTICE.txt");
    if path.exists() {
        return;
    }
    let _ = std::fs::write(
        path,
        "adel VIctory sends anonymous performance reports to https://rxnd.de/vi \
         so frame-rate regressions can be compared across builds. Reports contain \
         PC specs, driver versions, screen size, temperatures where available, \
         the exact FPS timeline, heavily compressed game-window screenshots every \
         ten seconds for the first ten minutes, and bounded Victory/title crash \
         and game log files from the run. They contain no game packages, usernames \
         or network addresses; embedded paths in logs are redacted on the server. \
         Your machine has an anonymous random key so runs from different PCs can \
         be told apart.\n\n\
         Opt out with VICTORY_TELEMETRY=0, [telemetry] enabled = false in \
         victory.toml, or victory run --no-telemetry. Local copies of every report \
         stay in this directory.\n",
    );
}

#[cfg(windows)]
fn load_token(dir: &std::path::Path, machine_id: &str) -> Option<String> {
    let path = dir.join("token");
    if let Ok(existing) = std::fs::read_to_string(&path) {
        let token = existing.trim();
        if !token.is_empty() {
            return Some(token.to_owned());
        }
    }
    let alias: String = std::env::var("VICTORY_TELEMETRY_ALIAS")
        .unwrap_or_default()
        .chars()
        .take(64)
        .collect();
    match windows::register(machine_id, &alias) {
        Ok(token) => {
            let _ = std::fs::write(&path, &token);
            Some(token)
        }
        Err(error) => {
            tracing::warn!(error = %error, "telemetry registration failed; keeping local reports only");
            None
        }
    }
}

#[cfg(windows)]
fn post(path: &str, body: &str, token: Option<&str>) {
    match windows::post(path, body, token) {
        Ok((status, _)) if (200..300).contains(&status) => {}
        Ok((status, response)) => {
            tracing::warn!(status, response = %response, "telemetry upload rejected")
        }
        Err(error) => tracing::warn!(error = %error, "telemetry upload failed; local report kept"),
    }
}

/// Shot index 1..=60 for elapsed times in `[0, 600)` seconds (one every 10 s).
fn screenshot_slot(elapsed: Duration) -> Option<u8> {
    if elapsed >= Duration::from_secs(600) {
        return None;
    }
    u8::try_from(elapsed.as_secs() / 10 + 1).ok()
}

#[cfg(windows)]
fn maybe_capture_screenshot(
    dir: &std::path::Path,
    run_id: &str,
    token: Option<&str>,
    elapsed: Duration,
    next_slot: &mut u8,
) {
    let Some(slot) = screenshot_slot(elapsed) else {
        return;
    };
    if slot < *next_slot {
        return;
    }
    *next_slot = slot.saturating_add(1);
    let Some(jpeg) = windows::capture_game_jpeg() else {
        return;
    };
    let name = format!("{slot:02}.jpg");
    let shots = dir.join(format!("screenshots-{run_id}"));
    let _ = std::fs::create_dir_all(&shots);
    let path = shots.join(&name);
    if std::fs::write(&path, &jpeg).is_err() {
        return;
    }
    let Some(token) = token else {
        return;
    };
    let remote = format!("/vi/v1/runs/{run_id}/screenshots/{name}");
    match windows::post_jpeg(&remote, &jpeg, token) {
        Ok((status, _)) if (200..300).contains(&status) => {}
        Ok((status, response)) => {
            tracing::warn!(status, response = %response, shot = %name, "telemetry screenshot rejected")
        }
        Err(error) => {
            tracing::warn!(error = %error, shot = %name, "telemetry screenshot upload failed; local copy kept")
        }
    }
}

#[cfg(windows)]
fn record(options: Options, stop: Receiver<()>) {
    let dir = options.dir;
    notice(&dir);
    // Older installs derived this ID from CPUID, which can vary with the
    // CPU running the thread. Recover the ID bound to an existing token.
    let identity_path = dir.join("machine_id");
    let saved = std::fs::read_to_string(&identity_path)
        .ok()
        .filter(|id| (16..=128).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_hexdigit()));
    let machine_id = saved.unwrap_or_else(|| {
        let token = std::fs::read_to_string(dir.join("token")).ok();
        let recovered = token
            .as_deref()
            .and_then(|t| windows::token_machine_id(t.trim()));
        let id = recovered.clone().unwrap_or_else(windows::machine_id);
        if token.is_none() || recovered.is_some() {
            let _ = std::fs::write(&identity_path, &id);
        }
        id
    });
    let token = load_token(&dir, &machine_id);
    let id = run_id();
    let started = iso_utc();
    let specs = windows::specs();
    let (width, height) = windows::display();
    let fork = Json::Obj(vec![
        ("name", Json::string("adel")),
        ("git_sha", Json::string(git_sha())),
        ("version", Json::string(env!("CARGO_PKG_VERSION"))),
    ]);
    let manifest = Json::Obj(vec![
        (
            "os",
            Json::Obj(vec![
                ("name", Json::string(specs.os_name)),
                ("version", Json::string(specs.os_build)),
            ]),
        ),
        (
            "cpu",
            Json::Obj(vec![
                ("name", Json::string(specs.cpu_name)),
                ("cores", Json::Int(u64::from(specs.cpu_cores))),
            ]),
        ),
        (
            "gpu",
            Json::Obj(vec![
                ("name", Json::string(specs.gpu_name)),
                ("driver", Json::string(specs.gpu_driver)),
                ("vram_mb", Json::Int(specs.vram_mb)),
            ]),
        ),
        (
            "display",
            Json::Obj(vec![
                ("width", Json::Int(u64::from(width))),
                ("height", Json::Int(u64::from(height))),
            ]),
        ),
        ("ram_total_mb", Json::Int(specs.ram_total_mb)),
        (
            "title",
            Json::Obj(vec![
                ("hash", Json::string(options.title_hash)),
                ("console", Json::string(options.console)),
            ]),
        ),
    ]);

    let local_manifest = Json::Obj(vec![
        ("schema", Json::Int(1)),
        ("run_id", Json::string(&id)),
        ("machine_id", Json::string(&machine_id)),
        ("started_at", Json::string(&started)),
        ("fork", fork.clone()),
        ("manifest", manifest.clone()),
    ]);
    let _ = std::fs::write(
        dir.join(format!("manifest-{id}.json")),
        local_manifest.to_string(),
    );

    let ping = Json::Obj(vec![
        ("schema", Json::Int(1)),
        ("run_id", Json::string(&id)),
        ("machine_id", Json::string(&machine_id)),
        ("fork", Json::string("adel")),
        ("git_sha", Json::string(git_sha())),
        ("version", Json::string(env!("CARGO_PKG_VERSION"))),
        ("started_at", Json::string(&started)),
    ]);
    if let Some(token) = &token {
        post("/vi/v1/ping", &ping.to_string(), Some(token));
    }

    let samples_path = dir.join(format!("samples-{id}.jsonl"));
    let file = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&samples_path);
    let Ok(file) = file else {
        tracing::warn!("telemetry samples file could not be opened; no local report");
        return;
    };
    let mut writer = std::io::BufWriter::new(file);
    let mut samples: Vec<Sample> = Vec::new();
    let mut cursor = metrics::Cursor::default();
    let began = Instant::now();
    let mut next_shot = 1u8;
    let mut uploaded_logs = std::collections::BTreeSet::new();
    maybe_capture_screenshot(&dir, &id, token.as_deref(), Duration::ZERO, &mut next_shot);
    let mut last_log_scan = Instant::now() - Duration::from_secs(30);
    loop {
        match stop.recv_timeout(Duration::from_millis(100)) {
            Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        maybe_capture_screenshot(&dir, &id, token.as_deref(), began.elapsed(), &mut next_shot);
        if last_log_scan.elapsed() >= Duration::from_secs(30) {
            if let Some(token) = token.as_deref() {
                upload_logs(
                    &dir,
                    &id,
                    token,
                    &options.log_roots,
                    began,
                    &mut uploaded_logs,
                );
            }
            last_log_scan = Instant::now();
        }
        let windows = metrics::drain(&mut cursor);
        let (cpu_pct, gpu_pct, ram_mb) = windows::rates();
        let (cpu_temp_c, cpu_temp_reason, gpu_temp_c, gpu_temp_reason) = windows::temperatures();
        for metrics in windows {
            let sample = Sample {
                t_ms: began.elapsed().as_secs_f64() * 1000.0,
                fps: metrics.fps,
                ms: metrics.ms,
                draws: metrics.draws,
                cpu_ms: metrics.cpu_ms,
                gpu_ms: metrics.gpu_ms,
                cpu_pct,
                gpu_pct,
                ram_mb,
                cpu_temp_c,
                cpu_temp_reason: cpu_temp_reason.clone(),
                gpu_temp_c,
                gpu_temp_reason: gpu_temp_reason.clone(),
            };
            let _ = std::io::Write::write_all(
                &mut writer,
                format!("{}\n", sample.to_json()).as_bytes(),
            );
            samples.push(sample);
        }
        if !samples.is_empty() && samples.len().is_multiple_of(5) {
            let _ = std::io::Write::flush(&mut writer);
        }
    }
    let _ = std::io::Write::flush(&mut writer);

    let summary = summary::compute(&samples);
    let _ = std::fs::write(
        dir.join(format!("summary-{id}.json")),
        summary.to_json().to_string(),
    );
    let bundle = Json::Obj(vec![
        ("schema", Json::Int(1)),
        ("run_id", Json::string(&id)),
        ("machine_id", Json::string(&machine_id)),
        ("started_at", Json::string(&started)),
        ("fork", fork),
        ("manifest", manifest),
        (
            "samples",
            Json::Arr(samples.iter().map(Sample::to_json).collect()),
        ),
        ("events", Json::Arr(vec![])),
        ("summary", summary.to_json()),
    ]);
    if let Some(token) = &token {
        post("/vi/v1/ingest", &bundle.to_string(), Some(token));
        upload_logs(
            &dir,
            &id,
            token,
            &options.log_roots,
            began,
            &mut uploaded_logs,
        );
    }
    tracing::info!(run = %id, samples = samples.len(), "telemetry report written to {}", dir.display());
}

#[cfg(windows)]
fn upload_logs(
    dir: &std::path::Path,
    run_id: &str,
    token: &str,
    roots: &[PathBuf],
    began: Instant,
    already: &mut std::collections::BTreeSet<String>,
) {
    let started = SystemTime::now()
        .checked_sub(began.elapsed())
        .unwrap_or(SystemTime::UNIX_EPOCH);
    let files = logs::collect(roots, started);
    if files.is_empty() {
        return;
    }
    let local = dir.join(format!("logs-{run_id}"));
    let _ = std::fs::create_dir_all(&local);
    for (name, path) in files {
        let key = format!("{name}:{}", path.metadata().map(|m| m.len()).unwrap_or(0));
        if !already.insert(key) {
            continue;
        }
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        let _ = std::fs::write(local.join(&name), &bytes);
        let remote = format!("/vi/v1/runs/{run_id}/logs/{name}");
        let content_type = if name.to_ascii_lowercase().ends_with(".dmp") {
            "application/octet-stream"
        } else {
            "text/plain; charset=utf-8"
        };
        match windows::post_bytes(&remote, &bytes, content_type, token) {
            Ok((status, _)) if (200..300).contains(&status) => {}
            Ok((status, response)) => {
                tracing::warn!(status, response = %response, log = %name, "telemetry log rejected")
            }
            Err(error) => {
                tracing::warn!(error = %error, log = %name, "telemetry log upload failed; local copy kept")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_id_is_server_compatible() {
        let id = run_id();
        assert!(id.len() <= 64);
        assert!(
            id.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        );
    }

    #[test]
    fn utc_stamp_is_well_formed() {
        let stamp = iso_utc();
        assert_eq!(stamp.len(), 20);
        assert!(stamp.ends_with('Z'));
        assert_eq!(stamp.as_bytes()[4], b'-');
    }

    #[test]
    fn civil_dates_cross_year_boundary() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(18_262), (2020, 1, 1));
    }

    #[test]
    fn screenshot_slots_cover_first_ten_minutes() {
        assert_eq!(screenshot_slot(Duration::from_secs(0)), Some(1));
        assert_eq!(screenshot_slot(Duration::from_secs(9)), Some(1));
        assert_eq!(screenshot_slot(Duration::from_secs(10)), Some(2));
        assert_eq!(screenshot_slot(Duration::from_secs(599)), Some(60));
        assert_eq!(screenshot_slot(Duration::from_secs(600)), None);
    }
}
