import re

with open("src/omen-gui/src/keyboardrgb.rs", "r") as f:
    content = f.read()

# 1. Disable 'Key' button for non-PerKey devices
old_sel_box = """    let btn_off = gtk::ToggleButton::builder().label("Off").build();
    let btn_key = gtk::ToggleButton::builder().label("Key").active(true).build();
    let btn_zone = gtk::ToggleButton::builder().label("Zone").build();
    let btn_all = gtk::ToggleButton::builder().label("All").build();"""

new_sel_box = """    let btn_off = gtk::ToggleButton::builder().label("Off").build();
    let btn_key = gtk::ToggleButton::builder().label("Key").active(detected_mode == KeyboardMode::PerKey).sensitive(detected_mode == KeyboardMode::PerKey).build();
    let btn_zone = gtk::ToggleButton::builder().label("Zone").active(detected_mode != KeyboardMode::PerKey).build();
    let btn_all = gtk::ToggleButton::builder().label("All").build();
    
    // Set initial sel_mode based on supported mode
    let sel_mode = Rc::new(RefCell::new(if detected_mode == KeyboardMode::PerKey { SelectionMode::Key } else { SelectionMode::Zone }));
"""

content = content.replace(old_sel_box, new_sel_box)

# Remove the old sel_mode declaration so it doesn't conflict
content = content.replace("    let sel_mode = Rc::new(RefCell::new(SelectionMode::Key));\n", "")

# 2. Add sel_mode_draw clone
old_clones = """    let keys_draw = keys_rc.clone();
    let selected_draw = selected_keys.clone();
    let hovered_draw = hovered_key.clone();
    let colors_draw = key_colors.clone();"""

new_clones = """    let keys_draw = keys_rc.clone();
    let selected_draw = selected_keys.clone();
    let hovered_draw = hovered_key.clone();
    let colors_draw = key_colors.clone();
    let sel_mode_draw = sel_mode.clone();"""

content = content.replace(old_clones, new_clones)

# 3. Add drawing logic for zones
# We will insert it at the end of the set_draw_func closure
old_end_draw = """            }
        }
    });

    let motion = gtk::EventControllerMotion::new();"""

new_end_draw = """            }
        }
        
        let active_sel = *sel_mode_draw.borrow();
        if active_sel == SelectionMode::Zone {
            // Draw zone bounding boxes
            for z in 1..=4 {
                let mut min_x = f64::MAX;
                let mut min_y = f64::MAX;
                let mut max_x = f64::MIN;
                let mut max_y = f64::MIN;
                let mut found = false;
                for k in keys_draw.iter() {
                    if get_zone_for_key(&k.name) == z {
                        if k.x < min_x { min_x = k.x; }
                        if k.y < min_y { min_y = k.y; }
                        if k.x + k.w > max_x { max_x = k.x + k.w; }
                        if k.y + k.h > max_y { max_y = k.y + k.h; }
                        found = true;
                    }
                }
                if found {
                    cr.set_source_rgba(1.0, 1.0, 1.0, 0.4);
                    cr.set_line_width(2.0);
                    // cr.set_dash(&[4.0, 4.0], 0.0);
                    draw_rounded_rect(cr, min_x - 2.0, min_y - 2.0, (max_x - min_x) + 4.0, (max_y - min_y) + 4.0, 6.0);
                    let _ = cr.stroke();
                }
            }
        } else if active_sel == SelectionMode::Key {
             // In Key mode, maybe draw a faint dash around all keys to signify they are individual?
             // Actually, the stroke on each key is enough, but we can make it slightly brighter if needed.
             // But let's leave it, the keys already have individual strokes.
        }
    });

    let motion = gtk::EventControllerMotion::new();"""

content = content.replace(old_end_draw, new_end_draw)

# 4. Add instructions label to bottom_box
old_bottom_box_start = """    bottom_box.append(&gtk::Label::builder().label("Color:").css_classes(["dim-label"]).build());
    bottom_box.append(&global_color_btn);"""

new_bottom_box_start = """    let instructions = gtk::Label::builder()
        .label("Pick a color first, then click on the keys/zones above to paint them.")
        .css_classes(["dim-label"])
        .margin_end(16)
        .build();
    bottom_box.append(&instructions);
    
    bottom_box.append(&gtk::Label::builder().label("Color:").css_classes(["dim-label"]).build());
    bottom_box.append(&global_color_btn);"""
content = content.replace(old_bottom_box_start, new_bottom_box_start)


with open("src/omen-gui/src/keyboardrgb.rs", "w") as f:
    f.write(content)
