#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrimaxLayout {
    /// 167 active LED slots (`0461:4E9B`, `4F03`, `4F11`, `4F1E`, `4ECB`, and default Primax fallback).
    Ralph,
    /// 168 active LED slots (`0461:4E9A`, OMEN 17 with left macro column).
    Cybug,
}

const RALPH_MAP: &[(usize, &[usize])] = &[
    // Row 0
    (0, &[0, 6]),
    (1, &[1, 7]),
    (2, &[2, 8]),
    (3, &[3, 9]),
    (4, &[4, 10]),
    (5, &[5, 11]),
    (6, &[60, 66]),
    (7, &[61, 67]),
    (8, &[62, 68]),
    (9, &[63, 69]),
    (10, &[64, 70]),
    (11, &[65, 71]),
    (12, &[120, 126]),
    (16, &[121, 127]),
    (13, &[122, 128]),
    (14, &[123, 129]),
    (15, &[124, 130]),
    (13, &[125, 131]), // PrtSc
    // Row 1
    (17, &[12, 18]),
    (18, &[13]),
    (19, &[14]),
    (20, &[15]),
    (21, &[16]),
    (22, &[17]),
    (23, &[72]),
    (24, &[73]),
    (25, &[74]),
    (26, &[75]),
    (27, &[76]),
    (28, &[77]),
    (29, &[132]),
    (30, &[133, 134, 135]),
    (31, &[138, 139]),
    (32, &[140, 141]),
    (33, &[142, 143]),
    // Row 2
    (35, &[24, 25]),
    (36, &[19]),
    (37, &[20]),
    (38, &[21]),
    (39, &[22]),
    (40, &[23]),
    (41, &[78]),
    (42, &[79]),
    (43, &[80]),
    (44, &[81]),
    (45, &[82]),
    (46, &[83]),
    (47, &[150]),
    (48, &[136, 137]),
    (49, &[144, 145]),
    (50, &[146, 147]),
    (51, &[148, 149]),
    // Row 3
    (53, &[30, 31, 32]),
    (54, &[26]),
    (55, &[27]),
    (56, &[28]),
    (57, &[29]),
    (58, &[84]),
    (59, &[85]),
    (60, &[86]),
    (61, &[87]),
    (62, &[88]),
    (63, &[89]),
    (64, &[151]),
    (65, &[152, 153, 154]),
    // Row 4
    (69, &[36, 37, 38]),
    (70, &[33]),
    (71, &[34]),
    (72, &[35]),
    (73, &[41]),
    (74, &[90]),
    (75, &[91]),
    (76, &[92]),
    (77, &[93]),
    (78, &[94]),
    (79, &[95]),
    (80, &[156, 157, 158]),
    (81, &[159]),
    // Row 5
    (86, &[42]),
    (87, &[43]),
    (88, &[44]),
    (89, &[45]),
    (90, &[96, 97, 98, 99, 100, 101]),
    (91, &[102]),
    (92, &[103]),
    (92, &[162]), // FnR/ISO
    (92, &[163]), // CtrlR
    (93, &[164]),
    (94, &[165]),
    (95, &[166]),
];

const CYBUG_MAP: &[(usize, &[usize])] = &[
    // Macro column
    (99, &[0, 6]),
    (100, &[12]),
    (101, &[18]),
    (102, &[24]),
    (103, &[30]),
    (103, &[42]), // P6
    // Row 0
    (0, &[1, 7]),
    (1, &[2, 8]),
    (2, &[3, 9]),
    (3, &[4, 10]),
    (4, &[5, 11]),
    (5, &[60, 66]),
    (6, &[61, 67]),
    (7, &[62, 68]),
    (8, &[63, 69]),
    (9, &[64, 70]),
    (10, &[65, 71]),
    (11, &[120, 126]),
    (12, &[121, 127]),
    (16, &[122, 128]),
    (13, &[123, 129]),
    (14, &[124, 130]),
    (15, &[125, 131]),
    (13, &[137, 143]), // PrtSc
    // Row 1
    (17, &[13]),
    (18, &[14]),
    (19, &[15]),
    (20, &[16]),
    (21, &[17]),
    (22, &[72]),
    (23, &[73]),
    (24, &[74]),
    (25, &[75]),
    (26, &[76]),
    (27, &[77]),
    (28, &[132]),
    (29, &[133]),
    (30, &[134, 135, 136]),
    (31, &[150]),
    (32, &[151]),
    (33, &[152]),
    // Row 2
    (35, &[19, 25, 31]),
    (36, &[20]),
    (37, &[21]),
    (38, &[22]),
    (39, &[23]),
    (40, &[78]),
    (41, &[79]),
    (42, &[80]),
    (43, &[81]),
    (44, &[82]),
    (45, &[83]),
    (46, &[138]),
    (47, &[139]),
    (48, &[140]),
    (49, &[153]),
    (50, &[154]),
    (51, &[155]),
    // Row 3
    (53, &[26, 27]),
    (54, &[28]),
    (55, &[29]),
    (56, &[35]),
    (57, &[84]),
    (58, &[85]),
    (59, &[86]),
    (60, &[87]),
    (61, &[88]),
    (62, &[89]),
    (63, &[144]),
    (64, &[145]),
    (65, &[146, 147, 148]),
    // Row 4
    (69, &[36, 37, 38]),
    (70, &[32]),
    (71, &[33]),
    (72, &[34]),
    (73, &[90]),
    (74, &[91]),
    (75, &[92]),
    (76, &[93]),
    (77, &[94]),
    (78, &[95]),
    (79, &[156]),
    (80, &[157, 158, 159]),
    (81, &[160]),
    // Row 5
    (86, &[43]),
    (87, &[44]),
    (88, &[45]),
    (89, &[46]),
    (90, &[96, 97, 98, 99, 100]),
    (91, &[101]),
    (92, &[107]),
    (92, &[163]), // FnR/ISO
    (92, &[164]), // CtrlR
    (93, &[165]),
    (94, &[166]),
    (95, &[167]),
];

impl PrimaxLayout {
    pub fn from_pid(device_pid: Option<u16>) -> Self {
        match device_pid {
            Some(0x4E9A) => Self::Cybug,
            _ => Self::Ralph,
        }
    }

    pub fn slot_map(self) -> &'static [(usize, &'static [usize])] {
        match self {
            Self::Ralph => RALPH_MAP,
            Self::Cybug => CYBUG_MAP,
        }
    }
}

/// Encode 104 logical key colors into Primax McuSDK2 180-slot R, G, B channel arrays.
pub fn encode_primax_channels(
    device_pid: Option<u16>,
    key_colors: &[(u8, u8, u8)],
) -> ([u8; 180], [u8; 180], [u8; 180]) {
    let mut r_leds = [0u8; 180];
    let mut g_leds = [0u8; 180];
    let mut b_leds = [0u8; 180];

    for &(key_slot, leds) in PrimaxLayout::from_pid(device_pid).slot_map() {
        let (r, g, b) = key_colors.get(key_slot).copied().unwrap_or((0, 0, 0));
        for &led in leds {
            if led < 180 {
                r_leds[led] = r;
                g_leds[led] = g;
                b_leds[led] = b;
            }
        }
    }

    (r_leds, g_leds, b_leds)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn ralph_and_cybug_have_zero_duplicate_led_ownership() {
        for layout in [PrimaxLayout::Ralph, PrimaxLayout::Cybug] {
            let map = layout.slot_map();
            assert!(!map.is_empty(), "{layout:?} map must not be empty");
            let mut seen = HashSet::new();
            for &(key_slot, leds) in map {
                assert!(key_slot < 104, "key_slot {key_slot} out of bounds");
                for &led in leds {
                    assert!(
                        seen.insert(led),
                        "duplicate LED ownership {led} in {layout:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn ralph_and_cybug_indices_within_180_and_null_slots_stay_zero() {
        let all_white = vec![(255u8, 255u8, 255u8); 104];

        let (r_r, g_r, b_r) = encode_primax_channels(Some(0x4F03), &all_white);
        let ralph_nulls: Vec<usize> = (39..=40)
            .chain(46..=59)
            .chain(104..=119)
            .chain(std::iter::once(155))
            .chain(160..=161)
            .chain(167..=179)
            .collect();
        for idx in ralph_nulls {
            assert_eq!(
                (r_r[idx], g_r[idx], b_r[idx]),
                (0, 0, 0),
                "Ralph null slot {idx} must remain (0,0,0)"
            );
        }
        // Ralph active slots should be lit
        assert_eq!((r_r[0], g_r[0], b_r[0]), (255, 255, 255));
        assert_eq!((r_r[166], g_r[166], b_r[166]), (255, 255, 255));

        let (r_c, g_c, b_c) = encode_primax_channels(Some(0x4E9A), &all_white);
        let cybug_nulls: Vec<usize> = (39..=41)
            .chain(47..=59)
            .chain(102..=106)
            .chain(108..=119)
            .chain(141..=142)
            .chain(std::iter::once(149))
            .chain(161..=162)
            .chain(168..=179)
            .collect();
        for idx in cybug_nulls {
            assert_eq!(
                (r_c[idx], g_c[idx], b_c[idx]),
                (0, 0, 0),
                "Cybug null slot {idx} must remain (0,0,0)"
            );
        }
        // Cybug macro P1(0,6) and Right(167) should be lit
        assert_eq!((r_c[0], g_c[0], b_c[0]), (255, 255, 255));
        assert_eq!((r_c[167], g_c[167], b_c[167]), (255, 255, 255));
    }

    #[test]
    fn ralph_slot_58_g_lights_led_84_only() {
        let mut keys = vec![(0u8, 0u8, 0u8); 104];
        keys[58] = (12, 34, 56);
        let (r, g, b) = encode_primax_channels(Some(0x4F03), &keys);
        assert_eq!((r[84], g[84], b[84]), (12, 34, 56));
        for led in 100..=102 {
            assert_eq!((r[led], g[led], b[led]), (0, 0, 0));
        }
    }

    #[test]
    fn multi_led_keys_light_all_assigned_slots() {
        let mut keys = vec![(0u8, 0u8, 0u8); 104];
        keys[30] = (10, 20, 30); // Backspace
        keys[69] = (40, 50, 60); // ShiftL
        keys[90] = (70, 80, 90); // Space
        keys[65] = (100, 110, 120); // Enter

        let (r, g, b) = encode_primax_channels(Some(0x4F03), &keys);
        for led in [133, 134, 135] {
            assert_eq!((r[led], g[led], b[led]), (10, 20, 30));
        }
        for led in [36, 37, 38] {
            assert_eq!((r[led], g[led], b[led]), (40, 50, 60));
        }
        for led in [96, 97, 98, 99, 100, 101] {
            assert_eq!((r[led], g[led], b[led]), (70, 80, 90));
        }
        for led in [152, 153, 154] {
            assert_eq!((r[led], g[led], b[led]), (100, 110, 120));
        }
    }
}
