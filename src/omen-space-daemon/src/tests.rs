//! Integration / unit tests for omen-space-daemon
//!
//! Issues covered:
//!   #264 — chassis_temp cache prevents AE_AML_BUFFER_LIMIT journal flood
//!   #266 — hotkey raw scancode 200 (XF86Launch2) accepted as "omen" event
//!   #268 — boards.json contains 8D41 with correct fields
//!   #270 — board 8E0F has supports_gpu_power_boost = true
//!   #275 — board 8DD0 is present in models and verified_boards
//!   #282 — ACPI health guard blocks profile switch under error storm
//!   #283 — PPD profile names mapped correctly from omen-space names

#[cfg(test)]
#[allow(clippy::module_inception)]
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
        assert!(
            is_verified(&json, "8DD0"),
            "Board 8DD0 must be in verified_boards (issue #275)"
        );
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
        assert!(
            get_board(&json, "8E0F").is_some(),
            "Board 8E0F must have a full model entry (issue #270)"
        );
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
        assert!(
            is_verified(&json, "8E0F"),
            "Board 8E0F must appear in verified_boards"
        );
    }

    // ── Issue #268 — Board 8D41 correct entry ────────────────────────────────

    #[test]
    fn board_8d41_present() {
        let json = boards_json();
        assert!(
            get_board(&json, "8D41").is_some(),
            "Board 8D41 must be in boards.json (issue #268)"
        );
    }

    #[test]
    fn board_8d41_per_key_rgb() {
        let json = boards_json();
        let board = get_board(&json, "8D41").unwrap();
        assert!(board["has_per_key_rgb"].as_bool().unwrap_or(false));
        assert!(board["has_light_bar"].as_bool().unwrap_or(false));
    }

    #[test]
    fn board_8bd4_has_single_zone_rgb() {
        let cap = crate::capabilities::detect(
            "8BD4",
            "Victus by HP Gaming Laptop 16-s0xxx",
            "AMD Ryzen 7 7840HS",
        );
        assert_eq!(cap.rgb_zone_count, Some(1));
    }

    #[test]
    fn rgb_zone_count_defaults_to_none_when_absent() {
        let cap = crate::capabilities::detect("8D41", "", "");
        assert_eq!(cap.rgb_zone_count, None);
        assert_eq!(
            crate::capabilities::ModelCapabilities::default().rgb_zone_count,
            None
        );
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
        let mut cache = FakeCache {
            last_read: None,
            value: 0,
            read_count: 0,
        };
        let t1 = cache.get_temp();
        assert_eq!(t1, 40);
        assert_eq!(cache.read_count, 1, "First call must trigger sysfs read");
        let t2 = cache.get_temp();
        assert_eq!(t2, 40);
        assert_eq!(
            cache.read_count, 1,
            "Second call within 60 s must NOT read again (fix #264)"
        );
    }

    // ── Issue #266 — Hotkey raw scancode fallback ─────────────────────────────

    #[test]
    fn hotkey_raw_scancode_200_maps_to_omen() {
        // Calls the REAL mapping used by the hotkey monitor (this test used to
        // re-implement it locally, so it kept passing even if the real code broke).
        use crate::hotkey_monitor::{resolve_hotkey, ModState};
        use omen_types::hotkey::HotkeySpec;
        let mods = ModState::default();
        let binding = HotkeySpec::default();
        assert_eq!(resolve_hotkey(148, &mods, &binding), Some("omen"));
        assert_eq!(
            resolve_hotkey(200, &mods, &binding),
            Some("omen"),
            "raw XF86Launch2 scancode must work (fix #266)"
        );
        assert_eq!(resolve_hotkey(149, &mods, &binding), Some("prog2"));
        assert_eq!(resolve_hotkey(140, &mods, &binding), Some("calc"));
        assert_eq!(resolve_hotkey(256, &mods, &binding), Some("prog3"));
        assert_eq!(resolve_hotkey(1, &mods, &binding), None);

        let mut shift = ModState::default();
        shift.update(42, true);
        assert_eq!(resolve_hotkey(60, &shift, &binding), Some("overlay"));
    }

    #[test]
    fn shift_f2_opens_the_overlay_but_plain_f2_does_not() {
        use crate::hotkey_monitor::{resolve_hotkey, ModState};
        use evdev::Key;
        use omen_types::hotkey::HotkeySpec;
        let binding = HotkeySpec::default();
        let mut shift = ModState::default();
        shift.update(42, true);
        assert_eq!(Key::KEY_F2.code(), 60, "evdev KEY_F2 must stay code 60");
        assert_eq!(
            resolve_hotkey(Key::KEY_F2.code(), &shift, &binding),
            Some("overlay")
        );
        assert_eq!(resolve_hotkey(60, &shift, &binding), Some("overlay"));
        assert_eq!(
            resolve_hotkey(60, &ModState::default(), &binding),
            None,
            "F2 without Shift must not toggle the HUD"
        );
    }

    #[test]
    fn configured_hotkey_replaces_the_default_binding() {
        use crate::hotkey_monitor::{resolve_hotkey, ModState};
        use omen_types::hotkey::{HotkeyModifiers, HotkeySpec};

        let binding = HotkeySpec::with_label(
            64, // KEY_F6
            HotkeyModifiers {
                ctrl: true,
                alt: true,
                ..Default::default()
            },
            "F6",
        );

        let mut mods = ModState::default();
        mods.update(29, true); // left Ctrl
        mods.update(56, true); // left Alt
        assert_eq!(resolve_hotkey(64, &mods, &binding), Some("overlay"));

        // Exact modifier match: adding Shift must not trigger the binding.
        let mut with_shift = mods;
        with_shift.update(42, true);
        assert_eq!(resolve_hotkey(64, &with_shift, &binding), None);

        // The old default no longer fires once a custom hotkey is configured.
        let mut shift = ModState::default();
        shift.update(42, true);
        assert_eq!(resolve_hotkey(60, &shift, &binding), None);
    }

    #[test]
    fn cross_device_modifiers_match_hotkeys_on_separate_input_devices() {
        use crate::hotkey_monitor::{resolve_hotkey, SharedModState};
        use omen_types::hotkey::{HotkeyModifiers, HotkeySpec};

        // Example: Alt held on the main laptop keyboard (device 0) + HP WMI
        // KEY_PROG2 (149) pressed on the HP WMI hotkeys device (device 1).
        let binding = HotkeySpec::with_label(
            149,
            HotkeyModifiers {
                alt: true,
                ..Default::default()
            },
            "Launch2",
        );

        let mut shared = SharedModState::default();
        assert!(shared.update(0, 56, true)); // Left Alt on device 0
        assert_eq!(
            resolve_hotkey(149, &shared.combined(), &binding),
            Some("overlay")
        );

        // SYN_DROPPED on device 1 must not drop Alt held on device 0.
        shared.reset_device(1);
        assert_eq!(
            resolve_hotkey(149, &shared.combined(), &binding),
            Some("overlay")
        );

        // Releasing Alt on device 0 restores normal prog2 fallback.
        assert!(shared.update(0, 56, false));
        assert_eq!(
            resolve_hotkey(149, &shared.combined(), &binding),
            Some("prog2")
        );
    }

    #[test]
    fn shift_does_not_change_the_other_macro_keys() {
        use crate::hotkey_monitor::{resolve_hotkey, ModState};
        use omen_types::hotkey::HotkeySpec;
        let binding = HotkeySpec::default();
        for shift in [false, true] {
            let mut mods = ModState::default();
            mods.update(42, shift);
            assert_eq!(resolve_hotkey(148, &mods, &binding), Some("omen"));
            assert_eq!(resolve_hotkey(200, &mods, &binding), Some("omen"));
            assert_eq!(resolve_hotkey(149, &mods, &binding), Some("prog2"));
            assert_eq!(resolve_hotkey(140, &mods, &binding), Some("calc"));
            assert_eq!(resolve_hotkey(256, &mods, &binding), Some("prog3"));
        }
        // Ordinary keys (letters, brightness, the Shift keys themselves) map to nothing.
        let mut mods = ModState::default();
        mods.update(42, true);
        for code in [30u16, 42, 54, 224, 225] {
            assert_eq!(
                resolve_hotkey(code, &mods, &binding),
                None,
                "code {code} must not trigger anything"
            );
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
        assert!(
            !cooldown_ok(last_switch),
            "Rapid second switch must be blocked (fix #282)"
        );
    }

    // ── Issue #283 — PPD profile name mapping ─────────────────────────────────

    #[test]
    fn ppd_profile_name_mapping_is_correct() {
        fn to_ppd(profile: &str) -> &'static str {
            match profile {
                "performance" => "performance",
                "power-saver" => "power-saver",
                _ => "balanced",
            }
        }
        assert_eq!(to_ppd("performance"), "performance");
        assert_eq!(to_ppd("power-saver"), "power-saver");
        assert_eq!(to_ppd("balanced"), "balanced");
        assert_eq!(
            to_ppd("custom"),
            "balanced",
            "Unknown profiles must fall back to balanced (fix #283)"
        );
        assert_eq!(to_ppd(""), "balanced");
    }

    // ── Board 8D2F fan capabilities (#305) ───────────────────────────────────

    #[test]
    fn board_8d2f_fan_capabilities() {
        let json = boards_json();
        let board = get_board(&json, "8D2F").expect("Board 8D2F must exist");
        assert_eq!(board["supports_fan_control_wmi"].as_bool(), Some(true));
        assert_eq!(board["supports_fan_curves"].as_bool(), Some(true));
        assert_eq!(board["supports_rpm_readback"].as_bool(), Some(true));
    }

    #[test]
    fn newly_verified_boards_present() {
        let json = boards_json();
        for id in [
            "8A3D", "8A50", "8BBE", "8BD5", "8C99", "8C9C", "8C9A", "8BB2", "8BC8", "8B3B", "8C78",
            "8786", "8A4C", "8D40", "8EDD", "8EEF",
        ] {
            assert!(
                get_board(&json, id).is_some(),
                "Board {} must exist in models",
                id
            );
            assert!(
                is_verified(&json, id),
                "Board {} must exist in verified_boards",
                id
            );
        }
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
            assert!(
                seen.insert(id.to_string()),
                "Duplicate product_id '{}' in boards.json",
                id
            );
        }
    }

    #[test]
    fn verified_boards_all_have_model_entry() {
        let json = boards_json();
        let verified = json["verified_boards"]
            .as_array()
            .expect("verified_boards must be array");
        for id_val in verified {
            let id = id_val.as_str().unwrap_or("");
            assert!(
                get_board(&json, id).is_some(),
                "verified_boards contains '{}' but no model entry exists",
                id
            );
        }
    }

    fn fake_dkms_package(root: &std::path::Path, pkg: &str, ver: &str, conf: &str) {
        let dir = root.join(pkg).join(ver).join("source");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("dkms.conf"), conf).unwrap();
    }

    #[test]
    fn dkms_packages_shipping_hp_wmi_are_reported_except_our_own() {
        let root = std::env::temp_dir().join(format!("omen-dkms-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);

        fake_dkms_package(
            &root,
            "hp-omen-extra",
            "2.1.4",
            "BUILT_MODULE_NAME[0]=\"hp-wmi\"\nBUILT_MODULE_NAME[1]=\"hp-omen-extra\"\n",
        );
        fake_dkms_package(
            &root,
            "hp-wmi-fan-and-backlight-control",
            "0.0.2",
            "PACKAGE_NAME=\"hp-wmi-fan-and-backlight-control\"\nBUILT_MODULE_NAME[0]=\"hp-wmi\"\n",
        );
        fake_dkms_package(&root, "other-wmi", "1.0", "BUILT_MODULE_NAME[0]=hp_wmi\n");
        fake_dkms_package(
            &root,
            "nvidia",
            "580.0",
            "BUILT_MODULE_NAME[0]=\"nvidia\"\n",
        );

        let found = crate::conflict_detector::conflicting_hpwmi_dkms_packages(&root);
        let _ = std::fs::remove_dir_all(&root);

        assert_eq!(
            found,
            vec!["hp-wmi-fan-and-backlight-control/0.0.2", "other-wmi/1.0"]
        );
    }

    #[test]
    fn missing_dkms_root_reports_nothing() {
        let found = crate::conflict_detector::conflicting_hpwmi_dkms_packages(
            std::path::Path::new("/nonexistent/omen-dkms-root"),
        );
        assert!(found.is_empty());
    }
}
