#[inline(always)]
pub fn parse_kb(line: &str) -> f64 {
    let mut parts = line.split_whitespace();
    parts.next();
    parts.next().and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0)
}

/// Returns true if the current process is running as root (UID 0).
/// Reads /proc/self/status to avoid a libc dependency.
pub fn nix_is_root() -> bool {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("Uid:"))
                .and_then(|l| l.split_whitespace().nth(1))
                .and_then(|uid| uid.parse::<u32>().ok())
        })
        .map(|uid| uid == 0)
        .unwrap_or(false)
}

pub fn get_running_process_names() -> Vec<String> {
    let mut names = Vec::with_capacity(128);
    if let Ok(entries) = std::fs::read_dir("/proc") {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    if name.as_bytes().iter().all(|b| b.is_ascii_digit()) {
                        let comm_path = path.join("comm");
                        if let Ok(comm) = std::fs::read_to_string(comm_path) {
                            names.push(comm.trim().to_lowercase());
                        }
                    }
                }
            }
        }
    }
    names
}

