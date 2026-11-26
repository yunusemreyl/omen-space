use super::curves::{
    pct_to_pwm, pwm_enable_without_pwm_node, should_latch_max_pwm_enable,
    should_recover_ec_max_latch, should_trigger_thermal_protection,
};
use super::*;

fn test_state(hwmon: PathBuf) -> FanState {
    let mut max_speeds = HashMap::new();
    max_speeds.insert(1, 6000);
    max_speeds.insert(2, 6000);
    FanState {
        hwmon_path: Some(hwmon),
        minimum_manual_pwm: 50,
        zero_pct_manual_pwm: false,
        found_fans: vec![1, 2],
        max_speeds,
        fallback_paths: HashMap::new(),
        fan_count: 2,
        mode: "auto".to_string(),
        custom_curve_json: "[]".to_string(),
        custom_curve_gpu_json: String::new(),
        last_targets: HashMap::new(),
        thermal_protection_active: false,
        thermal_protection_enabled: true,
        thermal_protection_entered_at: std::time::Instant::now(),
        suppress_ec_notice: false,
        pre_protection_mode: None,
        manual_target_pct: None,
        last_keepalive: std::time::Instant::now(),
        auto_fan_activated_at: None,
        temp_history: std::collections::VecDeque::new(),
        last_auto_pct: 0,
        last_auto_pct_time: std::time::Instant::now(),
        perf_cooldown_start: None,
        last_perf_pct: 0,
        last_perf_pct_time: std::time::Instant::now(),
        last_power_profile_was_perf: false,
        last_written_duty: None,
        last_written_duty_time: None,
        last_hw_detect_attempt: std::time::Instant::now(),
        signal_ctx: None,
    }
}

#[test]
fn balanced_auto_curve_stays_below_max_boost_under_90c_load() {
    let pct_85 = evaluate_step(&default_auto_step_curve(), 85.0);
    let pct_89 = evaluate_step(&default_auto_step_curve(), 89.0);
    assert!(
        pct_85 < 95.0,
        "85C should not request 100% max latch ({})",
        pct_85
    );
    assert!(
        pct_89 < 95.0,
        "89C should not request 100% max latch ({})",
        pct_89
    );
    assert_eq!(evaluate_step(&default_auto_step_curve(), 93.0), 100.0);
}

#[test]
fn should_latch_max_pwm_enable_avoids_sticky_latch_in_curve_modes() {
    assert!(!should_latch_max_pwm_enable("auto", false, 100, true));
    assert!(!should_latch_max_pwm_enable("custom", false, 100, true));
    assert!(!should_latch_max_pwm_enable(
        "performance",
        false,
        100,
        true
    ));
    assert!(should_latch_max_pwm_enable("max", false, 100, true));
    assert!(should_latch_max_pwm_enable("auto", true, 100, true));
    assert!(should_latch_max_pwm_enable("auto", false, 100, false));
}

#[tokio::test]
async fn curve_at_100pct_uses_manual_255_when_pwm1_exists_no_max_latch() {
    let dir = std::env::temp_dir().join(format!("omen-fan-manual100-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("pwm1_enable"), "1").unwrap();
    std::fs::write(dir.join("pwm1"), "128").unwrap();
    let mut state = test_state(dir.clone());
    state.mode = "custom".to_string();

    assert!(write_pwm_duty(&mut state, 100).await);
    let enable_val = std::fs::read_to_string(dir.join("pwm1_enable")).unwrap();
    let pwm_val = std::fs::read_to_string(dir.join("pwm1")).unwrap();
    let _ = std::fs::remove_dir_all(&dir);

    assert_eq!(
        enable_val, "1",
        "custom 100% must stay in pwm1_enable=1 (#395)"
    );
    assert_eq!(pwm_val, "255");
}

#[tokio::test]
async fn explicit_max_mode_sets_pwm_enable_zero_and_leave_max_boost_resets_via_auto() {
    let dir = std::env::temp_dir().join(format!("omen-fan-maxleave-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("pwm1_enable"), "1").unwrap();
    std::fs::write(dir.join("pwm1"), "128").unwrap();
    let mut state = test_state(dir.clone());
    state.mode = "max".to_string();

    assert!(write_pwm_duty(&mut state, 100).await);
    assert_eq!(
        std::fs::read_to_string(dir.join("pwm1_enable")).unwrap(),
        "0"
    );

    // Now exit max boost directly via leave_max_boost: must write "2" (AUTO) to reset EC latch
    leave_max_boost(&dir).await;
    let reset_val = std::fs::read_to_string(dir.join("pwm1_enable")).unwrap();
    let _ = std::fs::remove_dir_all(&dir);

    assert_eq!(
        reset_val, "2",
        "leave_max_boost must transition 0 -> 2 (#395)"
    );
}

#[tokio::test]
async fn leave_max_boost_writes_2_even_when_pwm_enable_is_not_0() {
    let dir = std::env::temp_dir().join(format!("omen-fan-leave-force-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("pwm1_enable"), "1").unwrap();

    leave_max_boost(&dir).await;
    let val = std::fs::read_to_string(dir.join("pwm1_enable")).unwrap();
    let _ = std::fs::remove_dir_all(&dir);

    assert_eq!(
        val, "2",
        "leave_max_boost must write 2 without skipping when readback is not 0"
    );
}

#[tokio::test]
async fn set_mode_from_max_to_custom_writes_safe_initial_duty() {
    let dir = std::env::temp_dir().join(format!("omen-fan-max2custom-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("pwm1_enable"), "0").unwrap();
    std::fs::write(dir.join("pwm1"), "255").unwrap();
    let mut state = test_state(dir.clone());
    state.mode = "max".to_string();

    assert!(FanService::set_mode_internal(&mut state, "custom").await);
    let enable_val = std::fs::read_to_string(dir.join("pwm1_enable")).unwrap();
    let pwm_val = std::fs::read_to_string(dir.join("pwm1")).unwrap();
    let _ = std::fs::remove_dir_all(&dir);

    assert_eq!(enable_val, "1");
    let expected_duty = pct_to_pwm(38).max(state.minimum_manual_pwm).to_string();
    assert_eq!(
        pwm_val, expected_duty,
        "leaving max to manual must seed low safe duty immediately"
    );
}

#[test]
fn curve_modes_delegate_to_firmware_only_without_pwm_node() {
    for mode in ["auto", "performance", "custom"] {
        assert!(
            curve_mode_delegates_to_firmware(mode, true, false),
            "{}",
            mode
        );
        assert!(
            !curve_mode_delegates_to_firmware(mode, true, true),
            "{}",
            mode
        );
        assert!(
            !curve_mode_delegates_to_firmware(mode, false, false),
            "{}",
            mode
        );
    }
    for mode in ["max", "ec"] {
        assert!(
            !curve_mode_delegates_to_firmware(mode, true, false),
            "{}",
            mode
        );
    }
}

#[test]
fn pwm_enable_without_pwm_node_never_requests_manual() {
    assert_eq!(pwm_enable_without_pwm_node(100), 0);
    for pct in [0, 1, 35, 75, 99] {
        assert_eq!(pwm_enable_without_pwm_node(pct), 2, "pct {}", pct);
    }
}

#[tokio::test]
async fn regression_max_boost_released_without_pwm_node() {
    let dir = std::env::temp_dir().join(format!("omen-fan-nopwm-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("pwm1_enable"), "2").unwrap();
    let mut state = test_state(dir.clone());

    assert!(write_pwm_duty(&mut state, 100).await);
    let at_max = std::fs::read_to_string(dir.join("pwm1_enable")).unwrap();
    assert!(!state.last_targets.is_empty());

    assert!(write_pwm_duty(&mut state, 75).await);
    let after_cooldown = std::fs::read_to_string(dir.join("pwm1_enable")).unwrap();
    let targets_cleared = state.last_targets.is_empty();

    assert!(write_pwm_duty(&mut state, 0).await);
    let at_zero = std::fs::read_to_string(dir.join("pwm1_enable")).unwrap();
    let _ = std::fs::remove_dir_all(dir);

    assert_eq!(at_max, "0", "100% must still engage max boost");
    assert_eq!(
        after_cooldown, "2",
        "a sub-100% request must release max boost to firmware control"
    );
    assert!(
        targets_cleared,
        "stale max-boost targets must be dropped once firmware owns the fans"
    );
    assert_eq!(at_zero, "2");
}

#[tokio::test]
async fn regression_preserve_working_25_percent_request() {
    let dir = std::env::temp_dir().join(format!("omen-review-fan-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("pwm1"), "0").unwrap();
    std::fs::write(dir.join("pwm1_enable"), "1").unwrap();
    let mut state = test_state(dir.clone());
    state.mode = "custom".to_string();

    assert!(write_pwm_duty(&mut state, 25).await);
    let actual = std::fs::read_to_string(dir.join("pwm1")).unwrap();
    let _ = std::fs::remove_dir_all(dir);
    assert_eq!(
        actual, "64",
        "a known-working 25% target should remain 25% instead of a global board-independent 35% override"
    );
}

#[test]
fn thermal_protection_requires_three_consecutive_samples_above_95() {
    let mut ticks = 0;
    assert!(!should_trigger_thermal_protection(97.0, &mut ticks));
    assert_eq!(ticks, 1);
    assert!(!should_trigger_thermal_protection(88.0, &mut ticks));
    assert_eq!(ticks, 0);
    assert!(!should_trigger_thermal_protection(96.0, &mut ticks));
    assert!(!should_trigger_thermal_protection(96.0, &mut ticks));
    assert!(should_trigger_thermal_protection(96.0, &mut ticks));
    ticks = 0;
    assert!(should_trigger_thermal_protection(102.0, &mut ticks));
}

#[tokio::test]
async fn independent_fan_targets_write_fan1_and_fan2_targets() {
    let dir = std::env::temp_dir().join(format!("omen-fan-targets-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("pwm1_enable"), "1").unwrap();
    std::fs::write(dir.join("pwm1"), "0").unwrap();
    std::fs::write(dir.join("fan1_target"), "0").unwrap();
    std::fs::write(dir.join("fan2_target"), "0").unwrap();
    let mut state = test_state(dir.clone());

    assert!(write_fan_targets(&mut state, 50, 80).await);
    let f1 = std::fs::read_to_string(dir.join("fan1_target")).unwrap();
    let f2 = std::fs::read_to_string(dir.join("fan2_target")).unwrap();
    let _ = std::fs::remove_dir_all(dir);

    assert_eq!(f1, "3000");
    assert_eq!(f2, "4800");
}

#[test]
fn select_cpu_temp_paths_prefers_package_sensor_over_core_and_acpi_spikes() {
    let candidates = vec![
        (
            "acpitz".into(),
            "".into(),
            "".into(),
            PathBuf::from("/sys/class/hwmon/hwmon0/temp1_input"),
        ),
        (
            "coretemp".into(),
            "".into(),
            "".into(),
            PathBuf::from("/sys/class/hwmon/hwmon3/temp1_input"),
        ),
        (
            "coretemp".into(),
            "".into(),
            "".into(),
            PathBuf::from("/sys/class/hwmon/hwmon3/temp2_input"),
        ),
        (
            "spd5118".into(),
            "".into(),
            "".into(),
            PathBuf::from("/sys/class/hwmon/hwmon4/temp1_input"),
        ),
        (
            "nvme".into(),
            "".into(),
            "".into(),
            PathBuf::from("/sys/class/hwmon/hwmon1/temp1_input"),
        ),
    ];
    let picked = select_cpu_temp_paths(&candidates);
    assert_eq!(
        picked,
        vec![PathBuf::from("/sys/class/hwmon/hwmon3/temp1_input")]
    );
}

#[test]
fn ec_max_latch_detector_triggers_on_8c77_cool_idle_mismatch_and_ignores_real_load() {
    let mut ticks = 0;
    for _ in 0..7 {
        assert!(!should_recover_ec_max_latch(
            "auto", false, 58.0, 85, 5700, 2000, 6000, &mut ticks
        ));
    }
    assert!(should_recover_ec_max_latch(
        "auto", false, 58.0, 85, 5700, 2000, 6000, &mut ticks
    ));

    // Must reset immediately under high load/temp or high commanded PWM
    assert!(!should_recover_ec_max_latch(
        "auto", false, 84.0, 85, 5700, 2000, 6000, &mut ticks
    ));
    assert_eq!(ticks, 0);
    assert!(!should_recover_ec_max_latch(
        "auto", false, 58.0, 220, 5700, 5200, 6000, &mut ticks
    ));
}

#[tokio::test]
async fn disabling_thermal_protection_restores_previous_fan_mode() {
    let dir = std::env::temp_dir().join(format!("omen-fan-tprestore-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("pwm1_enable"), "0").unwrap();
    std::fs::write(dir.join("pwm1"), "255").unwrap();
    let mut state = test_state(dir.clone());
    state.mode = "max".to_string();
    state.thermal_protection_active = true;
    state.pre_protection_mode = Some("auto".to_string());

    FanService::apply_thermal_protection_toggle(&mut state, false).await;
    let _ = std::fs::remove_dir_all(&dir);

    assert!(!state.thermal_protection_enabled);
    assert!(!state.thermal_protection_active);
    assert_eq!(state.mode, "auto");
}
