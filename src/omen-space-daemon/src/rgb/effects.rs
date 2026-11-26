use super::key_layout::slot_coords;

/// Returns true when the animation loop should render & stream software frames
/// rather than letting `hw_effects` autonomous MCU animations hold the device.
pub fn should_stream_software_frame(hw_effects: bool, has_mcu_effect: bool) -> bool {
    !hw_effects || !has_mcu_effect
}

/// Compute animation color per zone or column.
#[allow(clippy::too_many_arguments)]
pub fn compute_anim_color(
    mode: &str,
    step: f64,
    eff_idx: usize,
    zone_count: usize,
    r1: u8,
    g1: u8,
    b1: u8,
    r2: u8,
    g2: u8,
    b2: u8,
) -> (u8, u8, u8) {
    use std::f64::consts::PI;
    let count = zone_count.max(1) as f64;
    match mode {
        "wave" => {
            let phase = step + (eff_idx as f64 * (2.0 * PI / count));
            let factor = (phase.sin() * 0.5) + 0.5;
            let r = (r1 as f64 * factor + r2 as f64 * (1.0 - factor)) as u8;
            let g = (g1 as f64 * factor + g2 as f64 * (1.0 - factor)) as u8;
            let b = (b1 as f64 * factor + b2 as f64 * (1.0 - factor)) as u8;
            (r, g, b)
        }
        "cycle" => {
            let hue = step;
            let r = ((hue.sin() * 127.0) + 128.0) as u8;
            let g = (((hue + 2.0 * PI / 3.0).sin() * 127.0) + 128.0) as u8;
            let b = (((hue + 4.0 * PI / 3.0).sin() * 127.0) + 128.0) as u8;
            (r, g, b)
        }
        "rainbow" | "wave_rainbow" => {
            let hue = step + (eff_idx as f64 * (2.0 * PI / count));
            let r = ((hue.sin() * 127.0) + 128.0) as u8;
            let g = (((hue + 2.0 * PI / 3.0).sin() * 127.0) + 128.0) as u8;
            let b = (((hue + 4.0 * PI / 3.0).sin() * 127.0) + 128.0) as u8;
            (r, g, b)
        }
        "breathing" | "pulse" => {
            let factor = (step.sin() * 0.5) + 0.5;
            (
                (r1 as f64 * factor) as u8,
                (g1 as f64 * factor) as u8,
                (b1 as f64 * factor) as u8,
            )
        }
        "blinking" => {
            let on = (step * 2.0).sin() >= 0.0;
            if on {
                (r1, g1, b1)
            } else {
                (0, 0, 0)
            }
        }
        "marquee" => {
            let head = (step * 2.5) % count;
            let direct = (eff_idx as f64 - head).abs();
            let dist = direct.min(count - direct);
            let width = (count * 0.28).max(1.2);
            let factor = (1.0 - (dist / width)).clamp(0.0, 1.0);
            let bg = 0.06;
            let blend = bg + (1.0 - bg) * factor;
            (
                (r1 as f64 * blend) as u8,
                (g1 as f64 * blend) as u8,
                (b1 as f64 * blend) as u8,
            )
        }
        "chase" => {
            let pos = (step * 2.0) as usize % zone_count.max(1);
            let factor = if eff_idx == pos { 1.0 } else { 0.15 };
            (
                (r1 as f64 * factor) as u8,
                (g1 as f64 * factor) as u8,
                (b1 as f64 * factor) as u8,
            )
        }
        "sparkle" => {
            use std::collections::hash_map::DefaultHasher;
            use std::hash::{Hash, Hasher};
            let mut h = DefaultHasher::new();
            (eff_idx as u64 + step as u64).hash(&mut h);
            let val = h.finish();
            let factor = if val.is_multiple_of(4) {
                0.1 + (val % 90) as f64 / 100.0
            } else {
                0.2
            };
            (
                (r1 as f64 * factor) as u8,
                (g1 as f64 * factor) as u8,
                (b1 as f64 * factor) as u8,
            )
        }
        "candle" => {
            let noise = (step.sin() * 0.3) + ((step * 2.3).sin() * 0.15);
            let factor = (0.6 + noise).clamp(0.3, 1.0);
            (
                (r1 as f64 * factor) as u8,
                (g1 as f64 * 0.6 * factor) as u8,
                (b1 as f64 * 0.2 * factor) as u8,
            )
        }
        "aurora" => {
            let hs = (step * 0.3) + (eff_idx as f64 * 0.5);
            let r = ((hs.sin() * 40.0) + 40.0) as u8;
            let g = (((hs + 1.0).cos() * 100.0) + 120.0) as u8;
            let b = (((hs + 2.0).sin() * 90.0) + 140.0) as u8;
            (r, g, b)
        }
        "disco" => {
            let beat = (step * 1.5) as u64;
            let seed = beat.wrapping_add(eff_idx as u64);
            let lcg = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let r = (lcg >> 56) as u8;
            let g = (lcg >> 48) as u8;
            let b = (lcg >> 40) as u8;
            (r, g, b)
        }
        "gradient" => {
            let blend = ((step + eff_idx as f64 * 0.7).sin() * 0.5) + 0.5;
            let r = (r1 as f64 * (1.0 - blend) + r2 as f64 * blend) as u8;
            let g = (g1 as f64 * (1.0 - blend) + g2 as f64 * blend) as u8;
            let b = (b1 as f64 * (1.0 - blend) + b2 as f64 * blend) as u8;
            (r, g, b)
        }
        _ => (r1, g1, b1),
    }
}

/// Render a full 104-slot per-key frame for any animated or interactive RGB mode.
/// `recent_keys` supplies `(x, y, age_secs)` for `reactive` and `ripple`.
#[allow(clippy::too_many_arguments)]
pub fn render_per_key_frame(
    mode: &str,
    step: f64,
    direction: &str,
    primary: (u8, u8, u8),
    secondary: (u8, u8, u8),
    scaler: f64,
    recent_keys: &[(f64, f64, f64)],
) -> Vec<String> {
    let (r1, g1, b1) = primary;
    let (r2, g2, b2) = secondary;
    let mut out = Vec::with_capacity(104);

    for i in 0..104 {
        let (x, y) = slot_coords(i);
        let (mut rf, mut gf, mut bf) = (0.0f64, 0.0f64, 0.0f64);

        match mode {
            "reactive" => {
                let mut max_intensity = 0.0f64;
                for &(kx, ky, age) in recent_keys {
                    let dist = ((kx - x).powi(2) + (ky - y).powi(2)).sqrt();
                    if dist < 0.85 {
                        let intensity = (1.0 - age * 1.5).clamp(0.0, 1.0);
                        max_intensity = max_intensity.max(intensity);
                    }
                }
                rf = r1 as f64 * max_intensity;
                gf = g1 as f64 * max_intensity;
                bf = b1 as f64 * max_intensity;
            }
            "ripple" => {
                for &(kx, ky, age) in recent_keys {
                    let dist = ((kx - x).powi(2) + (ky - y).powi(2)).sqrt();
                    let radius = age * 14.0;
                    let thickness = 1.6;
                    if (dist - radius).abs() < thickness {
                        let intensity = (1.0 - age * 0.8).clamp(0.0, 1.0);
                        rf += r1 as f64 * intensity;
                        gf += g1 as f64 * intensity;
                        bf += b1 as f64 * intensity;
                    }
                }
                rf = rf.clamp(0.0, 255.0);
                gf = gf.clamp(0.0, 255.0);
                bf = bf.clamp(0.0, 255.0);
            }
            "starlight" => {
                use std::collections::hash_map::DefaultHasher;
                use std::hash::{Hash, Hasher};
                let mut h = DefaultHasher::new();
                let slow_step = (step * 2.0) as u64;
                (slow_step, i).hash(&mut h);
                if h.finish().is_multiple_of(20) {
                    rf = r1 as f64;
                    gf = g1 as f64;
                    bf = b1 as f64;
                }
            }
            "raindrop" => {
                let col = x.round();
                let drop_pos = (step * 4.0 + col * 2.7) % 8.0;
                if (y - drop_pos).abs() < 0.9 {
                    rf = r1 as f64;
                    gf = g1 as f64;
                    bf = b1 as f64;
                }
            }
            _ => {
                let col = (x.round() as usize).min(19);
                let eff_idx = if direction == "rtl" { 19 - col } else { col };
                let (rc, gc, bc) =
                    compute_anim_color(mode, step, eff_idx, 20, r1, g1, b1, r2, g2, b2);
                rf = rc as f64;
                gf = gc as f64;
                bf = bc as f64;
            }
        }

        out.push(format!(
            "{:02X}{:02X}{:02X}",
            (rf * scaler) as u8,
            (gf * scaler) as u8,
            (bf * scaler) as u8
        ));
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hw_effects_only_blocks_streaming_when_mcu_supports_the_mode() {
        // #385: Even if hw_effects is ON, marquee/reactive/ripple have no MCU opcode so software streaming MUST run.
        assert!(!should_stream_software_frame(true, true));
        assert!(should_stream_software_frame(true, false));
        assert!(should_stream_software_frame(false, true));
        assert!(should_stream_software_frame(false, false));
    }

    #[test]
    fn marquee_moves_lit_band_across_keyboard_columns() {
        let f_t0 = render_per_key_frame("marquee", 0.0, "ltr", (0, 128, 255), (0, 0, 0), 1.0, &[]);
        let f_t1 = render_per_key_frame("marquee", 2.0, "ltr", (0, 128, 255), (0, 0, 0), 1.0, &[]);
        assert_ne!(f_t0, f_t1, "marquee frame must advance over time (#385)");
        // At t=0 (head=0), Esc (slot 0, x=0.5) should be brighter than Enter (slot 65, x=13.87).
        assert_ne!(f_t0[0], f_t0[65], "marquee must light a partial band, not uniform static");
    }

    #[test]
    fn blinking_toggles_between_lit_and_off() {
        // Compute at phase >0 vs <0
        let lit = compute_anim_color("blinking", 0.5, 0, 4, 200, 100, 50, 0, 0, 0);
        let dark = compute_anim_color("blinking", 2.0, 0, 4, 200, 100, 50, 0, 0, 0);
        assert_eq!(lit, (200, 100, 50));
        assert_eq!(dark, (0, 0, 0));
    }

    #[test]
    fn reactive_lights_exact_pressed_physical_key_and_leaves_distant_keys_off() {
        let w_pos = slot_coords(37); // W
        let frame = render_per_key_frame(
            "reactive",
            0.0,
            "ltr",
            (255, 0, 0),
            (0, 0, 0),
            1.0,
            &[(w_pos.0, w_pos.1, 0.05)],
        );
        assert_ne!(frame[37], "000000", "pressed W key must light up (#385)");
        assert_eq!(frame[85], "000000", "distant numpad Enter must remain off");
    }

    #[test]
    fn ripple_expands_ring_over_time_around_pressed_key() {
        let s_pos = slot_coords(55); // S
        let early = render_per_key_frame(
            "ripple",
            0.0,
            "ltr",
            (0, 255, 128),
            (0, 0, 0),
            1.0,
            &[(s_pos.0, s_pos.1, 0.02)],
        );
        let later = render_per_key_frame(
            "ripple",
            0.0,
            "ltr",
            (0, 255, 128),
            (0, 0, 0),
            1.0,
            &[(s_pos.0, s_pos.1, 0.45)],
        );
        assert_ne!(early[55], "000000", "ring center lights up immediately");
        assert_eq!(early[60], "000000", "key J far away not yet reached");
        assert_ne!(later[60], "000000", "expanding ring reaches J at t=0.45s (#385)");
    }
}
