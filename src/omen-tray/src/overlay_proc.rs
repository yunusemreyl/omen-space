//! Liveness detection for the quick HUD overlay (`omen-overlay`).
//!
//! The tray toggles the overlay: if one is running it is closed, otherwise a new one
//! is started. `pgrep -x omen-overlay` cannot be used for that decision because it
//! also matches *defunct* (zombie) processes. A dead overlay whose parent never
//! called `wait()` makes the tray think the HUD is already open, so the hotkey only
//! ever "closes" a corpse and nothing appears. This module reads `/proc` directly and
//! only reports processes that are really alive and owned by the current user.

use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

/// Split the contents of `/proc/<pid>/stat` into `(comm, state)`.
///
/// `comm` sits between the first `(` and the **last** `)` because the executable name
/// may itself contain spaces and parentheses; the one-character state follows it.
pub fn parse_stat(stat: &str) -> Option<(&str, char)> {
    let open = stat.find('(')?;
    let close = stat.rfind(')')?;
    if close <= open {
        return None;
    }
    let comm = &stat[open + 1..close];
    let state = stat[close + 1..].trim_start().chars().next()?;
    Some((comm, state))
}

/// A process counts as alive unless it is a zombie (`Z`) or already dead (`X`/`x`).
fn is_alive_state(state: char) -> bool {
    !matches!(state, 'Z' | 'X' | 'x')
}

/// PIDs of live processes named `name` and owned by `uid`, found under `proc_root`
/// (normally `/proc`; a different root lets the tests use a fake tree).
///
/// `comm` is truncated to 15 bytes by the kernel; `omen-overlay` is shorter than that.
pub fn find_live_pids(proc_root: &Path, name: &str, uid: u32) -> Vec<u32> {
    let mut pids = Vec::new();
    let Ok(entries) = fs::read_dir(proc_root) else {
        return pids;
    };
    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let Some(pid) = file_name.to_str().and_then(|s| s.parse::<u32>().ok()) else {
            continue;
        };
        let dir = entry.path();
        let Ok(stat) = fs::read_to_string(dir.join("stat")) else {
            continue;
        };
        let Some((comm, state)) = parse_stat(&stat) else {
            continue;
        };
        if comm != name || !is_alive_state(state) {
            continue;
        }
        // /proc/<pid> is owned by the process's effective uid.
        if matches!(fs::metadata(&dir), Ok(md) if md.uid() == uid) {
            pids.push(pid);
        }
    }
    pids.sort_unstable();
    pids
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::process::Command;
    use std::time::{Duration, Instant};

    fn current_uid() -> u32 {
        // SAFETY: getuid() has no preconditions and cannot fail.
        unsafe { libc::getuid() }
    }

    #[test]
    fn parses_a_normal_stat_line() {
        let s = "1234 (omen-overlay) S 1 1234 1234 0 -1 4194560";
        assert_eq!(parse_stat(s), Some(("omen-overlay", 'S')));
    }

    #[test]
    fn parses_a_zombie_stat_line() {
        let s = "14644 (omen-overlay) Z 14631 14631 14631 0 -1 4228620";
        assert_eq!(parse_stat(s), Some(("omen-overlay", 'Z')));
    }

    #[test]
    fn comm_may_contain_spaces_and_parentheses() {
        let s = "77 (weird ) name) R 1 77 77 0 -1 0";
        assert_eq!(parse_stat(s), Some(("weird ) name", 'R')));
    }

    #[test]
    fn rejects_garbage() {
        assert_eq!(parse_stat(""), None);
        assert_eq!(parse_stat("no parens here"), None);
        assert_eq!(parse_stat(") ("), None);
        assert_eq!(parse_stat("1 (x)"), None); // no state character
    }

    #[test]
    fn only_live_processes_of_the_right_name_and_user_match() {
        let root: PathBuf = std::env::temp_dir().join(format!("omen-tray-proc-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        for (pid, stat) in [
            ("100", "100 (omen-overlay) S 1 100 100 0 -1 0"),   // live overlay -> match
            ("101", "101 (omen-overlay) Z 1 101 101 0 -1 0"),   // zombie overlay -> ignored
            ("102", "102 (omen-gui) S 1 102 102 0 -1 0"),       // other program -> ignored
            ("103", "103 (omen-overlay) R 1 103 103 0 -1 0"),   // running overlay -> match
            ("104", "104 (omen-overlay) X 1 104 104 0 -1 0"),   // dead -> ignored
        ] {
            fs::create_dir_all(root.join(pid)).unwrap();
            fs::write(root.join(pid).join("stat"), stat).unwrap();
        }
        fs::create_dir_all(root.join("self")).unwrap(); // non-numeric entry is skipped

        assert_eq!(find_live_pids(&root, "omen-overlay", current_uid()), vec![100, 103]);
        // Directories created by this test belong to the current user, so another
        // uid must see nothing.
        assert!(find_live_pids(&root, "omen-overlay", current_uid() + 1).is_empty());

        let _ = fs::remove_dir_all(&root);
    }

    /// The actual regression: a real, un-reaped child is a zombie and must not count.
    #[test]
    fn a_real_zombie_is_not_reported_as_running() {
        let mut child = Command::new("sh").args(["-c", "exit 0"]).spawn().unwrap();
        let pid = child.id();
        let stat_path = format!("/proc/{pid}/stat");

        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let state = fs::read_to_string(&stat_path)
                .ok()
                .and_then(|s| parse_stat(&s).map(|(_, st)| st));
            if state == Some('Z') {
                break;
            }
            assert!(Instant::now() < deadline, "child never became a zombie");
            std::thread::sleep(Duration::from_millis(10));
        }

        // `pgrep -x sh` would match this pid; the zombie-aware check must not.
        let live = find_live_pids(Path::new("/proc"), "sh", current_uid());
        assert!(!live.contains(&pid), "zombie pid {pid} was reported as live");

        child.wait().unwrap();
    }
}
