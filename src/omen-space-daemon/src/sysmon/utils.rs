#[inline(always)]
pub fn parse_kb(line: &str) -> f64 {
    let mut parts = line.split_whitespace();
    parts.next();
    parts
        .next()
        .and_then(|v| v.parse::<f64>().ok())
        .unwrap_or(0.0)
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
