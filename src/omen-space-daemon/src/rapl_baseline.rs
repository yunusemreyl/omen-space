//! Persist the first observed power limits, without claiming firmware provenance.
use serde::{Deserialize, Serialize};
use std::io::{ErrorKind, Write};
use std::path::Path;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Provenance {
    FirstObserved,
    LegacyObserved,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Baseline {
    pub pl1_w: u32,
    pub pl2_w: u32,
    pub provenance: Provenance,
}

impl Baseline {
    fn valid(&self) -> bool {
        (5..=250).contains(&self.pl1_w) && (self.pl1_w..=250).contains(&self.pl2_w)
    }
}

fn legacy(raw: &str) -> Option<Baseline> {
    let value: serde_json::Value = serde_json::from_str(raw).ok()?;
    let baseline = Baseline {
        pl1_w: u32::try_from(value.get("boot_pl1_w")?.as_u64()?).ok()?,
        pl2_w: u32::try_from(value.get("boot_pl2_w")?.as_u64()?).ok()?,
        provenance: Provenance::LegacyObserved,
    };
    baseline.valid().then_some(baseline)
}

/// The boolean reports persistence success; a current-session baseline may still be useful
/// if the state directory is unavailable. No numeric fallback is ever called a baseline.
pub fn load_or_capture(
    state_file: &Path,
    same_boot_cache: &Path,
    legacy_file: &Path,
    boot_id: &str,
    observed: Option<(u32, u32)>,
) -> Option<(Baseline, bool)> {
    match std::fs::read_to_string(state_file) {
        Ok(raw) => {
            if let Ok(baseline) = serde_json::from_str::<Baseline>(&raw) {
                if baseline.valid() {
                    return Some((baseline, true));
                }
            }
            // Re-observing here could replace the original baseline with active overrides.
            log::warn!("Invalid RAPL baseline at {}; restore is unavailable", state_file.display());
            return None;
        }
        Err(error) if error.kind() != ErrorKind::NotFound => {
            log::warn!("Cannot read RAPL baseline at {}: {}", state_file.display(), error);
            return None;
        }
        Err(_) => {}
    }

    let same_boot = std::fs::read_to_string(same_boot_cache).ok().and_then(|raw| {
        let value: serde_json::Value = serde_json::from_str(&raw).ok()?;
        if boot_id.is_empty() || value["boot_id"].as_str() != Some(boot_id) {
            return None;
        }
        legacy(&raw)
    });
    let old = std::fs::read_to_string(legacy_file).ok().and_then(|raw| legacy(&raw));
    let first = observed.map(|(pl1_w, pl2_w)| Baseline {
        pl1_w,
        pl2_w,
        provenance: Provenance::FirstObserved,
    }).filter(Baseline::valid);
    let baseline = same_boot.or(old).or(first)?;

    let persist = || -> std::io::Result<()> {
        if let Some(parent) = state_file.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let temporary = state_file.with_extension(format!("tmp-{}", std::process::id()));
        let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(&temporary)?;
        let result = (|| {
            file.write_all(&serde_json::to_vec_pretty(&baseline)?)?;
            file.sync_all()?;
            std::fs::rename(&temporary, state_file)
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(temporary);
        }
        result
    };
    let persisted = match persist() {
        Ok(()) => true,
        Err(error) => {
            log::warn!("Could not persist observed RAPL baseline at {}: {}", state_file.display(), error);
            false
        }
    };
    Some((baseline, persisted))
}

#[cfg(test)]
mod tests {
    use super::*;
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    struct Tree(std::path::PathBuf);
    impl Tree {
        fn new() -> Self { let p=std::env::temp_dir().join(format!("omen-baseline-{}-{}",std::process::id(),NEXT.fetch_add(1,std::sync::atomic::Ordering::Relaxed)));std::fs::create_dir_all(&p).unwrap();Self(p) }
        fn load(&self, values:Option<(u32,u32)>) -> Option<(Baseline,bool)> {load_or_capture(&self.0.join("state/baseline.json"),&self.0.join("cache"),&self.0.join("legacy"),"boot1",values)}
    }
    impl Drop for Tree { fn drop(&mut self) {let _=std::fs::remove_dir_all(&self.0);} }
    #[test] fn first_observation_survives_restarts_without_becoming_firmware_defaults() {
        let t=Tree::new();let first=t.load(Some((75,105))).unwrap();assert!(first.1);
        assert_eq!(first.0.provenance,Provenance::FirstObserved);
        assert_eq!(t.load(Some((80,98))).unwrap().0,first.0);
    }
    #[test] fn same_boot_cache_migrates_with_honest_provenance() {
        let t=Tree::new();std::fs::write(t.0.join("cache"),r#"{"boot_id":"boot1","boot_pl1_w":80,"boot_pl2_w":98}"#).unwrap();
        let b=t.load(Some((75,115))).unwrap().0;assert_eq!(b.pl1_w,80);assert_eq!(b.provenance,Provenance::LegacyObserved);
    }
    #[test] fn invalid_persistent_baseline_does_not_recapture_active_overrides() {
        let t=Tree::new();std::fs::create_dir_all(t.0.join("state")).unwrap();
        std::fs::write(t.0.join("state/baseline.json"),"invalid").unwrap();
        assert_eq!(t.load(Some((80,98))),None);
        assert_eq!(std::fs::read_to_string(t.0.join("state/baseline.json")).unwrap(),"invalid");
    }
    #[test] fn missing_telemetry_has_no_invented_default() {assert_eq!(Tree::new().load(None),None);}
    #[test] fn stale_cache_does_not_override_current_observation() {
        let t=Tree::new();std::fs::write(t.0.join("cache"),r#"{"boot_id":"old","boot_pl1_w":80,"boot_pl2_w":98}"#).unwrap();
        assert_eq!(t.load(Some((75,105))).unwrap().0.pl1_w,75);
    }
    #[test] fn corrupt_out_of_range_and_wrapping_legacy_values_are_rejected() {
        assert!(legacy(r#"{"boot_pl1_w":4294967376,"boot_pl2_w":98}"#).is_none());
        for pair in [(0,100),(100,75),(75,251)] {assert!(Tree::new().load(Some(pair)).is_none());}
    }
    #[test] fn persistence_failure_is_reported_without_losing_session_observation() {
        let t=Tree::new();std::fs::create_dir_all(t.0.join("state").join(format!("baseline.tmp-{}",std::process::id()))).unwrap();
        let b=t.load(Some((75,105))).unwrap();assert!(!b.1);assert_eq!(b.0.pl2_w,105);
    }
}
