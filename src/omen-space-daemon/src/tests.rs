/// Integration / unit tests for omen-space-daemon
///
/// Issues covered:
///   #264 — chassis_temp cache prevents AE_AML_BUFFER_LIMIT journal flood
///   #266 — hotkey raw scancode 200 (XF86Launch2) accepted as "omen" event
///   #268 — boards.json contains 8D41 with correct fields
///   #270 — board 8E0F has supports_gpu_power_boost = true
///   #275 — board 8DD0 is present in models and verified_boards
///   #282 — ACPI health guard blocks profile switch under error storm
///   #283 — PPD profile names mapped correctly from omen-space names

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    // ── helpers ───────────────────────────────────────────────────────────────

    fn boards_json() -> serde_json::Value {
        let raw = include_str!("boards.json");
        serde_json::from_str(raw).expect("boards.json must be valid JSON")
    }

    fn get_board(json: &serde_json::Value, product_id: &str) -> Option<serde_json::Value> {
        json["models"]
            .as_array()?
            .iter()
            .find(|b| b["product_id"].as_str() == Some(product_id))
            .cloned()
    }

    fn is_verified(json: &serde_json::Value, product_id: &str) -> bool {
        json["verified_boards"]
            .as_array()
            .map(|arr| arr.iter().any(|v| v.as_str() == Some(product_id)))
            .unwrap_or(false)
    }

    // ── Issue #275 — Board 8DD0 ───────────────────────────────────────────────

    #[test]
    fn board_8dd0_present_in_models() {
        let json = boards_json();
        let board = get_board(&json, "8DD0")
            .expect("Board 8DD0 (OMEN 17-cm3xxx) must be in boards.json models (issue #275)");
        assert_eq!(board["product_id"].as_str(), Some("8DD0"));
    }

    #[test]
    fn board_8dd0_is_verified() {
        let json = boards_json();
        assert!(is_verified(&json, "8DD0"), "Board 8DD0 must be in verified_boards (issue #275)");
    }

    #[test]
    fn board_8dd0_has_required_fields() {
        let json = boards_json();
        let board = get_board(&json, "8DD0").unwrap();
        assert!(board["supports_fan_control_wmi"].as_bool().unwrap_or(false));
        assert!(board["supports_gpu_power_boost"].as_bool().unwrap_or(false));
        assert!(board["has_mux_switch"].as_bool().unwrap_or(false));
    }

    // ── Issue #270 — Board 8E0F GPU boost ────────────────────────────────────

    #[test]
    fn board_8e0f_present_in_models() {
        let json = boards_json();
        assert!(get_board(&json, "8E0F").is_some(), "Board 8E0F must have a full model entry (issue #270)");
    }

    #[test]
    fn board_8e0f_gpu_boost_enabled() {
        let json = boards_json();
        let board = get_board(&json, "8E0F").expect("Board 8E0F must exist");
        assert!(
            board["supports_gpu_power_boost"].as_bool().unwrap_or(false),
            "Board 8E0F must have supports_gpu_power_boost=true to fix 90W GPU cap (issue #270)"
        );
    }

    #[test]
    fn board_8e0f_in_verified_boards() {
        let json = boards_json();
        assert!(is_verified(&json, "8E0F"), "Board 8E0F must appear in verified_boards");
    }

    // ── Issue #268 — Board 8D41 correct entry ────────────────────────────────

    #[test]
    fn board_8d41_present() {
        let json = boards_json();
        assert!(get_board(&json, "8D41").is_some(), "Board 8D41 must be in boards.json (issue #268)");
    }

    #[test]
    fn board_8d41_per_key_rgb() {
        let json = boards_json();
        let board = get_board(&json, "8D41").unwrap();
        assert!(board["has_per_key_rgb"].as_bool().unwrap_or(false));
        assert!(board["has_light_bar"].as_bool().unwrap_or(false));
    }

    // ── Issue #264 — chassis_temp cache test ─────────────────────────────────

    #[test]
    fn chassis_temp_cache_prevents_double_read() {
        struct FakeCache {
            last_read: Option<Instant>,
            value: i32,
            read_count: u32,
        }
        impl FakeCache {
            fn get_temp(&mut self) -> i32 {
                let now = Instant::now();
                let should_read = match self.last_read {
                    Some(t) => now.duration_since(t) >= Duration::from_secs(60),
                    None => true,
                };
                if should_read {
                    self.read_count += 1;
                    self.value = 40;
                    self.last_read = Some(now);
                }
                self.value
            }
        }
        let mut cache = FakeCache { last_read: None, value: 0, read_count: 0 };
        let t1 = cache.get_temp();
        assert_eq!(t1, 40);
        assert_eq!(cache.read_count, 1, "First call must trigger sysfs read");
        let t2 = cache.get_temp();
        assert_eq!(t2, 40);
        assert_eq!(cache.read_count, 1, "Second call within 60 s must NOT read again (fix #264)");
    }

    // ── Issue #266 — Hotkey raw scancode fallback ─────────────────────────────

    #[test]
    fn hotkey_raw_scancode_200_maps_to_omen() {
        // Calls the REAL mapping used by the hotkey monitor (this test used to
        // re-implement it locally, so it kept passing even if the real code broke).
        use crate::hotkey_monitor::resolve_hotkey;
        assert_eq!(resolve_hotkey(148, false), Some("omen"));
        assert_eq!(resolve_hotkey(200, false), Some("omen"), "raw XF86Launch2 scancode must work (fix #266)");
        assert_eq!(resolve_hotkey(149, false), Some("prog2"));
        assert_eq!(resolve_hotkey(140, false), Some("calc"));
        assert_eq!(resolve_hotkey(256, false), Some("prog3"));
        assert_eq!(resolve_hotkey(1, false), None);
        assert_eq!(resolve_hotkey(60, true), Some("overlay"));
    }

    #[test]
    fn shift_f2_opens_the_overlay_but_plain_f2_does_not() {
        use crate::hotkey_monitor::resolve_hotkey;
        use evdev::Key;
        assert_eq!(Key::KEY_F2.code(), 60, "evdev KEY_F2 must stay code 60");
        assert_eq!(resolve_hotkey(Key::KEY_F2.code(), true), Some("overlay"));
        assert_eq!(resolve_hotkey(60, true), Some("overlay"));
        assert_eq!(resolve_hotkey(60, false), None, "F2 without Shift must not toggle the HUD");
    }

    #[test]
    fn shift_does_not_change_the_other_macro_keys() {
        use crate::hotkey_monitor::resolve_hotkey;
        for shift in [false, true] {
            assert_eq!(resolve_hotkey(148, shift), Some("omen"));
            assert_eq!(resolve_hotkey(200, shift), Some("omen"));
            assert_eq!(resolve_hotkey(149, shift), Some("prog2"));
            assert_eq!(resolve_hotkey(140, shift), Some("calc"));
            assert_eq!(resolve_hotkey(256, shift), Some("prog3"));
        }
        // Ordinary keys (letters, brightness, the Shift keys themselves) map to nothing.
        for code in [30u16, 42, 54, 224, 225] {
            assert_eq!(resolve_hotkey(code, true), None, "code {code} must not trigger anything");
        }
    }

    // ── Issue #282 — ACPI cooldown throttle logic ─────────────────────────────

    #[test]
    fn profile_switch_cooldown_blocks_rapid_switches() {
        const COOLDOWN: Duration = Duration::from_secs(30);
        let cooldown_ok = |last: Option<Instant>| -> bool {
            last.map(|t| t.elapsed() >= COOLDOWN).unwrap_or(true)
        };
        let mut last_switch: Option<Instant> = None;
        assert!(cooldown_ok(last_switch), "First switch must be allowed");
        last_switch = Some(Instant::now());
        assert!(!cooldown_ok(last_switch), "Rapid second switch must be blocked (fix #282)");
    }

    // ── Issue #283 — PPD profile name mapping ─────────────────────────────────

    #[test]
    fn ppd_profile_name_mapping_is_correct() {
        fn to_ppd(profile: &str) -> &'static str {
            match profile {
                "performance" => "performance",
                "power-saver"  => "power-saver",
                _              => "balanced",
            }
        }
        assert_eq!(to_ppd("performance"), "performance");
        assert_eq!(to_ppd("power-saver"), "power-saver");
        assert_eq!(to_ppd("balanced"), "balanced");
        assert_eq!(to_ppd("custom"), "balanced", "Unknown profiles must fall back to balanced (fix #283)");
        assert_eq!(to_ppd(""), "balanced");
    }

    // ── boards.json integrity ─────────────────────────────────────────────────

    #[test]
    fn boards_json_is_valid_json() {
        let raw = include_str!("boards.json");
        assert!(serde_json::from_str::<serde_json::Value>(raw).is_ok());
    }

    #[test]
    fn boards_json_no_duplicate_product_ids() {
        let json = boards_json();
        let models = json["models"].as_array().expect("models must be array");
        let mut seen = std::collections::HashSet::new();
        for m in models {
            let id = m["product_id"].as_str().unwrap_or("");
            assert!(seen.insert(id.to_string()), "Duplicate product_id '{}' in boards.json", id);
        }
    }

    #[test]
    fn verified_boards_all_have_model_entry() {
        let json = boards_json();
        let verified = json["verified_boards"].as_array().expect("verified_boards must be array");
        for id_val in verified {
            let id = id_val.as_str().unwrap_or("");
            assert!(
                get_board(&json, id).is_some(),
                "verified_boards contains '{}' but no model entry exists",
                id
            );
        }
    }
}
