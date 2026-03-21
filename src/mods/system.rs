use serde_json::{json, Value};
use std::cell::RefCell;
use std::fs;

thread_local! {
    static PREV_CPU: RefCell<Option<Vec<u64>>> = RefCell::new(None);
}

pub fn poll(_params: &serde_json::Map<String, Value>) -> Value {
    let cpu_percent = calc_cpu();

    // Memory
    let (mem_total, mem_avail, swap_total, swap_free) = parse_meminfo();
    let mem_used = mem_total.saturating_sub(mem_avail);
    let swap_used = swap_total.saturating_sub(swap_free);

    // Temperature
    let temp = fs::read_to_string("/sys/class/thermal/thermal_zone0/temp")
        .ok()
        .and_then(|s| s.trim().parse::<i64>().ok())
        .map(|t| t / 1000)
        .unwrap_or(0);

    // Load average
    let load = fs::read_to_string("/proc/loadavg")
        .ok()
        .and_then(|s| s.split_whitespace().next().map(String::from))
        .and_then(|s| s.parse::<f64>().ok())
        .unwrap_or(0.0);

    json!({
        "cpu_percent": cpu_percent,
        "mem_used_bytes": mem_used,
        "mem_total_bytes": mem_total,
        "swap_used_bytes": swap_used,
        "swap_total_bytes": swap_total,
        "temp": temp,
        "load": load,
    })
}

fn calc_cpu() -> u64 {
    let cur = match read_cpu_times() {
        Some(v) => v,
        None => return 0,
    };

    PREV_CPU.with(|prev| {
        let mut prev = prev.borrow_mut();
        let result = match prev.as_ref() {
            Some(old) => {
                let d: Vec<u64> = cur.iter().zip(old.iter()).map(|(c, o)| c - o).collect();
                let total: u64 = d.iter().sum();
                let idle = d.get(3).copied().unwrap_or(0);
                if total > 0 {
                    ((total - idle) * 100 / total) as u64
                } else {
                    0
                }
            }
            None => 0,
        };
        *prev = Some(cur);
        result
    })
}

fn read_cpu_times() -> Option<Vec<u64>> {
    let content = fs::read_to_string("/proc/stat").ok()?;
    let line = content.lines().find(|l| l.starts_with("cpu "))?;
    let vals: Vec<u64> = line
        .split_whitespace()
        .skip(1)
        .filter_map(|s| s.parse().ok())
        .collect();
    if vals.len() >= 4 { Some(vals) } else { None }
}

fn parse_meminfo() -> (u64, u64, u64, u64) {
    let content = match fs::read_to_string("/proc/meminfo") {
        Ok(s) => s,
        Err(_) => return (0, 0, 0, 0),
    };

    let mut mem_total = 0u64;
    let mut mem_avail = 0u64;
    let mut swap_total = 0u64;
    let mut swap_free = 0u64;

    for line in content.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 2 {
            continue;
        }
        let val: u64 = parts[1].parse().unwrap_or(0) * 1024; // kB to bytes
        match parts[0] {
            "MemTotal:" => mem_total = val,
            "MemAvailable:" => mem_avail = val,
            "SwapTotal:" => swap_total = val,
            "SwapFree:" => swap_free = val,
            _ => {}
        }
    }

    (mem_total, mem_avail, swap_total, swap_free)
}
