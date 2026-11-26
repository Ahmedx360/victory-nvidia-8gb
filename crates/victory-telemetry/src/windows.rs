//! Windows-only plumbing: anonymous identity, host specs, opportunistic
//! sensors and HTTPS uploads through WinHTTP (no extra dependencies).

use std::ffi::c_void;
use std::sync::{Mutex, OnceLock};

use windows_sys::Win32::Foundation::FILETIME;
use windows_sys::Win32::Networking::WinHttp::*;
use windows_sys::Win32::Storage::FileSystem::GetVolumeInformationW;
use windows_sys::Win32::System::Performance::*;
use windows_sys::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS_EX};
use windows_sys::Win32::System::Registry::*;
use windows_sys::Win32::System::SystemInformation::{
    GetSystemInfo, GlobalMemoryStatusEx, MEMORYSTATUSEX, SYSTEM_INFO,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetSystemTimes};
use windows_sys::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};

use crate::json::{Json, find_string};

const MB: f64 = 1024.0 * 1024.0;

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain([0]).collect()
}

fn registry_string(key: HKEY, path: &str, name: &str) -> Option<String> {
    let path = wide(path);
    let name = wide(name);
    let mut buf = [0u16; 256];
    let mut bytes = std::mem::size_of_val(&buf) as u32;
    // SAFETY: the buffer and lengths describe the same allocation.
    let rc = unsafe {
        RegGetValueW(
            key,
            path.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            buf.as_mut_ptr().cast(),
            &mut bytes,
        )
    };
    if rc != 0 {
        return None;
    }
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    let text = String::from_utf16_lossy(&buf[..len]).trim().to_owned();
    (!text.is_empty()).then_some(text)
}

/// A stable, anonymous machine key: CPU identity + Windows volume serial +
/// core count. No account name, path, serial of a peripheral, or MAC address.
pub fn machine_id() -> String {
    let mut seed = String::new();
    #[cfg(target_arch = "x86_64")]
    {
        let leaf0 = std::arch::x86_64::__cpuid(0);
        let max = leaf0.eax;
        if max >= 1 {
            let leaf1 = std::arch::x86_64::__cpuid(1);
            seed.push_str(&format!("{:08x}{:08x}", leaf1.eax, leaf1.ebx));
        }
        if max >= 0x8000_0004 {
            for leaf in [0x8000_0002, 0x8000_0003, 0x8000_0004] {
                let regs = std::arch::x86_64::__cpuid(leaf);
                seed.push_str(&format!(
                    "{:08x}{:08x}{:08x}{:08x}",
                    regs.eax, regs.ebx, regs.ecx, regs.edx
                ));
            }
        }
    }
    let root = wide("C:\\");
    let mut serial = 0u32;
    // SAFETY: only the serial out-parameter is requested; all other buffers are null.
    let ok = unsafe {
        GetVolumeInformationW(
            root.as_ptr(),
            std::ptr::null_mut(),
            0,
            &mut serial,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            0,
        )
    };
    if ok != 0 {
        seed.push_str(&format!(":{serial:08x}"));
    }
    // SAFETY: plain out-structure.
    let mut info: SYSTEM_INFO = unsafe { std::mem::zeroed() };
    // SAFETY: a plain out-parameter.
    unsafe { GetSystemInfo(&mut info) };
    seed.push_str(&format!(":{}", info.dwNumberOfProcessors));
    let digest = blake3::hash(seed.as_bytes());
    digest.to_hex()[..32].to_owned()
}

#[derive(Clone, Debug, Default)]
pub struct Specs {
    pub os_name: String,
    pub os_build: String,
    pub cpu_name: String,
    pub cpu_cores: u32,
    pub gpu_name: String,
    pub gpu_driver: String,
    pub vram_mb: u64,
    pub ram_total_mb: u64,
}

pub fn specs() -> Specs {
    let mut result = Specs {
        os_name: registry_string(
            HKEY_LOCAL_MACHINE,
            r"SOFTWARE\Microsoft\Windows NT\CurrentVersion",
            "ProductName",
        )
        .unwrap_or_default(),
        os_build: registry_string(
            HKEY_LOCAL_MACHINE,
            r"SOFTWARE\Microsoft\Windows NT\CurrentVersion",
            "DisplayVersion",
        )
        .unwrap_or_default(),
        cpu_name: registry_string(
            HKEY_LOCAL_MACHINE,
            r"HARDWARE\DESCRIPTION\System\CentralProcessor\0",
            "ProcessorNameString",
        )
        .unwrap_or_default(),
        ..Default::default()
    };
    // SAFETY: plain out-structure.
    let mut info: SYSTEM_INFO = unsafe { std::mem::zeroed() };
    // SAFETY: a plain out-parameter.
    unsafe { GetSystemInfo(&mut info) };
    result.cpu_cores = info.dwNumberOfProcessors;
    // SAFETY: plain out-structure with its length set.
    let mut memory: MEMORYSTATUSEX = unsafe { std::mem::zeroed() };
    memory.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
    // SAFETY: the length field matches the allocation.
    unsafe { GlobalMemoryStatusEx(&mut memory) };
    result.ram_total_mb = memory.ullTotalPhys / MB as u64;

    if let Ok(gpu) = gpu_description() {
        result.gpu_name = gpu.0;
        result.vram_mb = gpu.1 / (MB as u64);
    }
    result.gpu_driver = driver_version();
    result
}

fn gpu_description() -> Result<(String, u64), ()> {
    use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIFactory1};
    // SAFETY: a plain COM factory creation; the returned interface owns itself.
    let factory: IDXGIFactory1 = unsafe { CreateDXGIFactory1() }.map_err(|_| ())?;
    // SAFETY: index 0 is the default adapter; the interface is still alive.
    let adapter = unsafe { factory.EnumAdapters1(0) }.map_err(|_| ())?;
    // SAFETY: a plain description query on the live adapter.
    let desc = unsafe { adapter.GetDesc() }.map_err(|_| ())?;
    let len = desc
        .Description
        .iter()
        .position(|&c| c == 0)
        .unwrap_or(desc.Description.len());
    Ok((
        String::from_utf16_lossy(&desc.Description[..len])
            .trim()
            .to_owned(),
        desc.DedicatedVideoMemory as u64,
    ))
}

fn driver_version() -> String {
    const KEY: &str =
        r"SYSTEM\CurrentControlSet\Control\Class\{4d36e968-e325-11ce-bfc1-08002be10318}\0000";
    registry_string(HKEY_LOCAL_MACHINE, KEY, "DriverVersion").unwrap_or_default()
}

struct CpuTimes(u64, u64);

fn cpu_times() -> (u64, u64) {
    let zero = FILETIME {
        dwLowDateTime: 0,
        dwHighDateTime: 0,
    };
    let (mut idle, mut kernel, mut user) = (zero, zero, zero);
    // SAFETY: plain out-parameters.
    unsafe { GetSystemTimes(&mut idle, &mut kernel, &mut user) };
    let ft = |t: FILETIME| u64::from(t.dwHighDateTime) << 32 | u64::from(t.dwLowDateTime);
    (ft(idle), ft(kernel) + ft(user))
}

struct PdhCounter(*mut c_void, *mut c_void);

// SAFETY: the two handles are only used from the single recorder thread and
// are never cloned or dropped while a query is in progress.
unsafe impl Send for PdhCounter {}
unsafe impl Sync for PdhCounter {}

impl PdhCounter {
    fn new(path: &str) -> Option<Self> {
        let path = wide(path);
        let (mut query, mut counter) = (std::ptr::null_mut(), std::ptr::null_mut());
        // SAFETY: out handles and a NUL-terminated counter path.
        unsafe {
            if PdhOpenQueryW(std::ptr::null(), 0, &mut query) != 0 {
                return None;
            }
            if PdhAddEnglishCounterW(query, path.as_ptr(), 0, &mut counter) != 0 {
                PdhCloseQuery(query);
                return None;
            }
            PdhCollectQueryData(query);
        }
        Some(PdhCounter(query, counter))
    }

    fn values(&self) -> Option<Vec<f64>> {
        // SAFETY: the query and counter were created by `new` and kept open.
        unsafe {
            if PdhCollectQueryData(self.0) != 0 {
                return None;
            }
            let (mut size, mut count) = (0u32, 0u32);
            PdhGetFormattedCounterArrayW(
                self.1,
                PDH_FMT_DOUBLE,
                &mut size,
                &mut count,
                std::ptr::null_mut(),
            );
            if size == 0 {
                return None;
            }
            let mut buf = vec![0u64; (size as usize).div_ceil(8)];
            let items = buf.as_mut_ptr().cast::<PDH_FMT_COUNTERVALUE_ITEM_W>();
            if PdhGetFormattedCounterArrayW(self.1, PDH_FMT_DOUBLE, &mut size, &mut count, items)
                != 0
            {
                return None;
            }
            Some(
                (0..count as usize)
                    .map(|i| &*items.add(i))
                    .filter(|item| item.FmtValue.CStatus == 0 || item.FmtValue.CStatus == 1)
                    .map(|item| item.FmtValue.Anonymous.doubleValue)
                    .collect(),
            )
        }
    }
}

static CPU: OnceLock<Mutex<CpuTimes>> = OnceLock::new();
static GPU_3D: OnceLock<Option<PdhCounter>> = OnceLock::new();
static THERMAL: OnceLock<Option<PdhCounter>> = OnceLock::new();

/// CPU/GPU load and the process's working set, sampled once a second.
pub fn rates() -> (Option<f64>, Option<f64>, Option<f64>) {
    let (idle, total) = cpu_times();
    let state = CPU.get_or_init(|| Mutex::new(CpuTimes(0, 0)));
    let cpu = if let Ok(mut last) = state.lock() {
        let (di, dt) = (idle.saturating_sub(last.0), total.saturating_sub(last.1));
        *last = CpuTimes(idle, total);
        if dt > 0 {
            Some((dt - di.min(dt)) as f64 * 100.0 / dt as f64)
        } else {
            None
        }
    } else {
        None
    };

    let gpu = GPU_3D
        .get_or_init(|| PdhCounter::new(r"\GPU Engine(*engtype_3D)\Utilization Percentage"))
        .as_ref()
        .and_then(PdhCounter::values)
        .map(|values| values.into_iter().sum::<f64>().clamp(0.0, 100.0));

    let ram = unsafe {
        let mut pm = std::mem::zeroed::<PROCESS_MEMORY_COUNTERS_EX>();
        pm.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32;
        if GetProcessMemoryInfo(
            GetCurrentProcess(),
            std::ptr::from_mut(&mut pm).cast(),
            pm.cb,
        ) == 0
        {
            None
        } else {
            Some(pm.WorkingSetSize as f64 / MB)
        }
    };
    (cpu, gpu, ram)
}

/// CPU package temperature through Windows thermal zones, if the machine
/// exposes that counter. GPU temperature needs NVAPI, deliberately deferred.
pub fn temperatures() -> (Option<f64>, Option<String>, Option<f64>, Option<String>) {
    let counter =
        THERMAL.get_or_init(|| PdhCounter::new(r"\Thermal Zone Information(*)\Temperature"));
    let cpu = counter
        .as_ref()
        .and_then(PdhCounter::values)
        .and_then(|values| {
            values
                .into_iter()
                .filter(|v| v.is_finite() && *v > 0.0)
                .max_by(f64::total_cmp)
                .map(|raw| {
                    // The counter is usually Kelvin; some drivers report tenths.
                    if raw > 500.0 {
                        raw / 10.0 - 273.15
                    } else {
                        raw - 273.15
                    }
                })
        });
    let cpu = match cpu {
        Some(value) if value > -100.0 && value < 150.0 => (Some(value), None),
        _ => (None, Some("thermal zone sensor unavailable".to_owned())),
    };
    let gpu = (
        None,
        Some("NVAPI temperature reader not enabled in v1".to_owned()),
    );
    (cpu.0, cpu.1, gpu.0, gpu.1)
}

/// Primary monitor size in pixels.
pub fn display() -> (u32, u32) {
    // SAFETY: these indices only query metrics, no arguments are passed.
    unsafe {
        (
            GetSystemMetrics(SM_CXSCREEN) as u32,
            GetSystemMetrics(SM_CYSCREEN) as u32,
        )
    }
}

/// HTTPS POST through WinHTTP with the system trust store. Returns the status
/// code and body; network failures are returned as text, never panics.
pub fn post(path: &str, body: &str, token: Option<&str>) -> Result<(u16, String), String> {
    request("POST", path, body, token)
}

pub fn token_machine_id(token: &str) -> Option<String> {
    let (status, body) = request("GET", "/vi/v1/stats", "", Some(token)).ok()?;
    if status != 200 {
        return None;
    }
    find_string(&body, "machine_id")
}

fn request(
    method: &str,
    path: &str,
    body: &str,
    token: Option<&str>,
) -> Result<(u16, String), String> {
    let agent = wide("adel-VIctory-telemetry/1.0");
    let host = wide("rxnd.de");
    let verb = wide(method);
    let resource = wide(path);
    let mut headers = String::from("Content-Type: application/json\r\n");
    if let Some(token) = token {
        headers.push_str("Authorization: Bearer ");
        headers.push_str(token);
        headers.push_str("\r\n");
    }
    let headers = wide(&headers);

    // SAFETY: every handle is checked before use and closed before return.
    unsafe {
        let session = WinHttpOpen(
            agent.as_ptr(),
            WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
            std::ptr::null(),
            std::ptr::null(),
            0,
        );
        if session.is_null() {
            return Err("WinHttpOpen failed".into());
        }
        let connect = WinHttpConnect(session, host.as_ptr(), 443, 0);
        if connect.is_null() {
            WinHttpCloseHandle(session);
            return Err("WinHttpConnect failed".into());
        }
        let request = WinHttpOpenRequest(
            connect,
            verb.as_ptr(),
            resource.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            WINHTTP_FLAG_SECURE,
        );
        if request.is_null() {
            WinHttpCloseHandle(connect);
            WinHttpCloseHandle(session);
            return Err("WinHttpOpenRequest failed".into());
        }
        WinHttpSetTimeouts(request, 3000, 3000, 5000, 15_000);
        let ok = WinHttpSendRequest(
            request,
            headers.as_ptr(),
            u32::MAX,
            body.as_ptr().cast::<c_void>(),
            body.len() as u32,
            body.len() as u32,
            0,
        );
        if ok == 0 {
            WinHttpCloseHandle(request);
            WinHttpCloseHandle(connect);
            WinHttpCloseHandle(session);
            return Err("WinHttpSendRequest failed".into());
        }
        let ok = WinHttpReceiveResponse(request, std::ptr::null_mut());
        if ok == 0 {
            WinHttpCloseHandle(request);
            WinHttpCloseHandle(connect);
            WinHttpCloseHandle(session);
            return Err("WinHttpReceiveResponse failed".into());
        }
        let mut status = 0u32;
        let mut size = std::mem::size_of::<u32>() as u32;
        WinHttpQueryHeaders(
            request,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            std::ptr::null(),
            std::ptr::from_mut(&mut status).cast(),
            &mut size,
            std::ptr::null_mut(),
        );
        let mut response = Vec::new();
        loop {
            let mut chunk = [0u8; 4096];
            let mut read = 0u32;
            if WinHttpReadData(
                request,
                chunk.as_mut_ptr().cast(),
                chunk.len() as u32,
                &mut read,
            ) == 0
                || read == 0
            {
                break;
            }
            response.extend_from_slice(&chunk[..read as usize]);
            if response.len() > 64 * 1024 {
                break;
            }
        }
        WinHttpCloseHandle(request);
        WinHttpCloseHandle(connect);
        WinHttpCloseHandle(session);
        Ok((
            status as u16,
            String::from_utf8_lossy(&response).into_owned(),
        ))
    }
}

/// Registers this installation once and returns its per-machine token.
pub fn register(machine_id: &str, alias: &str) -> Result<String, String> {
    let body = Json::Obj(vec![
        ("schema", Json::Int(1)),
        ("machine_id", Json::string(machine_id)),
        ("alias", Json::string(alias)),
    ])
    .to_string();
    let (status, response) = post("/vi/v1/register", &body, None)?;
    if status != 200 && status != 201 {
        return Err(format!("register returned HTTP {status}: {response}"));
    }
    find_string(&response, "token").ok_or_else(|| "register response had no token".to_owned())
}
