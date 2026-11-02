//! Host resource stats for the dashboard: CPU load, memory and disk usage of
//! the machine this tool runs on (the TAK server's own box). Read straight
//! from `/proc` and a `statvfs` syscall -- no sampling loop, no extra daemon.

use std::ffi::CString;

/// A point-in-time snapshot of host resource usage.
pub struct SystemStats {
    /// Load averages (1 / 5 / 15 min) and the CPU count to read them against.
    pub load1: f64,
    pub load5: f64,
    pub load15: f64,
    pub ncpu: usize,
    pub mem_total_kb: u64,
    pub mem_available_kb: u64,
    /// Root filesystem, in bytes.
    pub disk_total: u64,
    pub disk_available: u64,
}

impl SystemStats {
    pub fn gather() -> Self {
        let (load1, load5, load15) = load_averages();
        let (mem_total_kb, mem_available_kb) = meminfo();
        let (disk_total, disk_available) = statvfs_bytes("/");
        Self {
            load1,
            load5,
            load15,
            ncpu: std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(1),
            mem_total_kb,
            mem_available_kb,
            disk_total,
            disk_available,
        }
    }

    pub fn mem_used_kb(&self) -> u64 {
        self.mem_total_kb.saturating_sub(self.mem_available_kb)
    }

    pub fn disk_used(&self) -> u64 {
        self.disk_total.saturating_sub(self.disk_available)
    }

    /// Memory used, 0-100.
    pub fn mem_percent(&self) -> f64 {
        percent(self.mem_used_kb(), self.mem_total_kb)
    }

    /// Disk used, 0-100.
    pub fn disk_percent(&self) -> f64 {
        percent(self.disk_used(), self.disk_total)
    }

    /// 1-min load as a fraction of total cores, 0-100 (clamped). A rough
    /// "how busy are the CPUs" readout.
    pub fn cpu_percent(&self) -> f64 {
        if self.ncpu == 0 {
            return 0.0;
        }
        (self.load1 / self.ncpu as f64 * 100.0).clamp(0.0, 100.0)
    }
}

fn percent(used: u64, total: u64) -> f64 {
    if total == 0 {
        0.0
    } else {
        (used as f64 / total as f64 * 100.0).clamp(0.0, 100.0)
    }
}

fn load_averages() -> (f64, f64, f64) {
    let text = std::fs::read_to_string("/proc/loadavg").unwrap_or_default();
    let mut it = text.split_whitespace();
    let parse = |v: Option<&str>| v.and_then(|s| s.parse().ok()).unwrap_or(0.0);
    (parse(it.next()), parse(it.next()), parse(it.next()))
}

/// `(MemTotal, MemAvailable)` in kB from `/proc/meminfo`.
fn meminfo() -> (u64, u64) {
    let text = std::fs::read_to_string("/proc/meminfo").unwrap_or_default();
    let field = |name: &str| -> u64 {
        text.lines()
            .find(|l| l.starts_with(name))
            .and_then(|l| l.split_whitespace().nth(1))
            .and_then(|v| v.parse().ok())
            .unwrap_or(0)
    };
    (field("MemTotal:"), field("MemAvailable:"))
}

/// `(total, available)` bytes for the filesystem containing `path`.
fn statvfs_bytes(path: &str) -> (u64, u64) {
    let Ok(c_path) = CString::new(path) else {
        return (0, 0);
    };
    // SAFETY: `statvfs` only writes into the zeroed struct we hand it, and
    // `c_path` is a valid NUL-terminated string that outlives the call.
    unsafe {
        let mut stat: libc::statvfs = std::mem::zeroed();
        if libc::statvfs(c_path.as_ptr(), &mut stat) != 0 {
            return (0, 0);
        }
        let frsize = stat.f_frsize as u64;
        let total = (stat.f_blocks as u64).saturating_mul(frsize);
        let available = (stat.f_bavail as u64).saturating_mul(frsize);
        (total, available)
    }
}

/// Human-readable bytes (GiB/MiB), one decimal.
pub fn human_bytes(bytes: u64) -> String {
    const GIB: f64 = 1024.0 * 1024.0 * 1024.0;
    const MIB: f64 = 1024.0 * 1024.0;
    let b = bytes as f64;
    if b >= GIB {
        format!("{:.1} GiB", b / GIB)
    } else {
        format!("{:.0} MiB", b / MIB)
    }
}
