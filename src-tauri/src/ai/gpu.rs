use std::sync::{OnceLock, RwLock};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Default)]
pub struct GpuSnapshot {
    pub name: Option<String>,
    pub util_pct: Option<f32>,
    pub mem_used_mb: Option<u64>,
    pub mem_total_mb: Option<u64>,
}

static HAS_NVIDIA: OnceLock<bool> = OnceLock::new();
static CACHED_GPU: RwLock<Option<(GpuSnapshot, Instant)>> = RwLock::new(None);

pub fn snapshot() -> GpuSnapshot {
    // 1. Fast path: return cached snapshot if fresh (< 5s)
    if let Ok(guard) = CACHED_GPU.read() {
        if let Some((snap, instant)) = guard.as_ref() {
            if instant.elapsed() < Duration::from_secs(5) {
                return snap.clone();
            }
        }
    }

    // 2. Slow path: probe without blocking caller for too long
    let snap = probe_all();
    if let Ok(mut guard) = CACHED_GPU.write() {
        *guard = Some((snap.clone(), Instant::now()));
    }
    snap
}

fn probe_all() -> GpuSnapshot {
    let has_nvidia = *HAS_NVIDIA.get_or_init(|| probe_nvidia().is_some());
    if has_nvidia {
        if let Some(s) = probe_nvidia() {
            return s;
        }
    }

    #[cfg(windows)]
    {
        if let Some(s) = probe_windows_cached() {
            return s;
        }
    }

    #[cfg(target_os = "linux")]
    {
        if let Some(s) = probe_linux() {
            return s;
        }
    }

    GpuSnapshot::default()
}

fn probe_nvidia() -> Option<GpuSnapshot> {
    let out = crate::proc::quiet_command("nvidia-smi")
        .args([
            "--query-gpu=utilization.gpu,memory.used,memory.total,name",
            "--format=csv,noheader,nounits",
        ])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let line = String::from_utf8_lossy(&out.stdout);
    let line = line.lines().next()?.trim();
    if line.is_empty() {
        return None;
    }
    let mut parts = line.split(',');
    let util = parts.next().and_then(|p| p.trim().parse::<f32>().ok());
    let used = parts.next().and_then(|p| p.trim().parse::<u64>().ok());
    let total = parts.next().and_then(|p| p.trim().parse::<u64>().ok());
    let name = parts
        .next()
        .map(|p| p.trim().to_string())
        .filter(|s| !s.is_empty());
    if util.is_none() && name.is_none() {
        return None;
    }
    Some(GpuSnapshot {
        name,
        util_pct: util,
        mem_used_mb: used,
        mem_total_mb: total,
    })
}

#[cfg(windows)]
static STATIC_WIN_GPU: OnceLock<Option<GpuSnapshot>> = OnceLock::new();

#[cfg(windows)]
fn probe_windows_cached() -> Option<GpuSnapshot> {
    let static_info = STATIC_WIN_GPU.get_or_init(probe_windows_static);
    static_info.clone()
}

#[cfg(windows)]
fn probe_windows_static() -> Option<GpuSnapshot> {
    let script = r#"
$ErrorActionPreference = 'SilentlyContinue'
$names = @(Get-CimInstance Win32_VideoController | Where-Object { $_.Name -and $_.PNPDeviceID } | ForEach-Object { $_.Name.Trim() } | Select-Object -Unique)
$name = ($names -join ' + ')
$totalBytes = 0
Get-ItemProperty 'HKLM:\SYSTEM\CurrentControlSet\Control\Class\{4d36e968-e325-11ce-bfc1-08002be10318}\*' |
  Where-Object { $_.'HardwareInformation.qwMemorySize' } |
  ForEach-Object { $n = [int64]$_.'HardwareInformation.qwMemorySize'; if ($n -gt $totalBytes) { $totalBytes = $n } }
Write-Output ("||{0}|{1}" -f [int]($totalBytes / 1MB), $name)
"#;
    let out = crate::proc::quiet_command("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .output()
        .ok()?;
    if !out.status.success() && out.stdout.is_empty() {
        return None;
    }
    parse_pipe_snapshot(&String::from_utf8_lossy(&out.stdout))
}

fn parse_pipe_snapshot(stdout: &str) -> Option<GpuSnapshot> {
    let line = stdout.lines().rev().find(|l| l.contains('|'))?.trim();
    let mut parts = line.splitn(4, '|');
    let util = parts.next().and_then(|p| p.trim().parse::<f32>().ok());
    let used = parts.next().and_then(|p| p.trim().parse::<u64>().ok());
    let total = parts.next().and_then(|p| p.trim().parse::<u64>().ok());
    let name = parts
        .next()
        .map(|p| p.trim().to_string())
        .filter(|s| !s.is_empty());
    if name.is_none() && util.is_none() && total.unwrap_or(0) == 0 {
        return None;
    }
    Some(GpuSnapshot {
        name,
        util_pct: util.filter(|v| *v >= 0.0),
        mem_used_mb: used.filter(|v| *v > 0),
        mem_total_mb: total.filter(|v| *v > 0),
    })
}

#[cfg(target_os = "linux")]
fn probe_linux() -> Option<GpuSnapshot> {
    if let Some(s) = probe_rocm() {
        return Some(s);
    }
    probe_linux_sysfs()
}

#[cfg(target_os = "linux")]
fn probe_rocm() -> Option<GpuSnapshot> {
    let out = crate::proc::quiet_command("rocm-smi")
        .args(["--showuse", "--showmeminfo", "vram", "--csv"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    // Best-effort: first numeric % and MiB we can find.
    let text = String::from_utf8_lossy(&out.stdout);
    let util = text
        .split(|c: char| !c.is_ascii_digit() && c != '.')
        .filter_map(|s| s.parse::<f32>().ok())
        .find(|v| *v <= 100.0);
    Some(GpuSnapshot {
        name: Some("AMD GPU".into()),
        util_pct: util,
        mem_used_mb: None,
        mem_total_mb: None,
    })
}

#[cfg(target_os = "linux")]
fn probe_linux_sysfs() -> Option<GpuSnapshot> {
    let out = crate::proc::quiet_command("sh")
        .args([
            "-c",
            "lspci 2>/dev/null | grep -iE 'vga|3d|display' | head -3 | sed 's/^[^ ]* //'",
        ])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let names: Vec<String> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();
    if names.is_empty() {
        return None;
    }
    Some(GpuSnapshot {
        name: Some(names.join(" + ")),
        util_pct: None,
        mem_used_mb: None,
        mem_total_mb: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_windows_pipe_line() {
        let s = parse_pipe_snapshot("12|1024|8192|Intel(R) UHD Graphics + AMD Radeon").unwrap();
        assert_eq!(s.util_pct, Some(12.0));
        assert_eq!(s.mem_used_mb, Some(1024));
        assert_eq!(s.mem_total_mb, Some(8192));
        assert!(s.name.unwrap().contains("Intel"));
    }

    #[test]
    fn parses_name_only_igpu() {
        let s = parse_pipe_snapshot("||0|Intel(R) UHD Graphics").unwrap();
        assert_eq!(s.util_pct, None);
        assert_eq!(s.mem_total_mb, None);
        assert_eq!(s.name.as_deref(), Some("Intel(R) UHD Graphics"));
    }
}
