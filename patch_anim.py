import re

with open("src/omen-gui/src/keyboardrgb.rs", "r") as f:
    content = f.read()

hsl_fn = """
fn hsl_to_rgb(h: f64, s: f64, l: f64) -> (f64, f64, f64) {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let mut h_prime = h / 60.0;
    while h_prime < 0.0 { h_prime += 6.0; }
    h_prime = h_prime % 6.0;
    let x = c * (1.0 - (h_prime % 2.0 - 1.0).abs());
    let m = l - c / 2.0;
    
    let (r1, g1, b1) = if h_prime >= 0.0 && h_prime < 1.0 { (c, x, 0.0) }
    else if h_prime >= 1.0 && h_prime < 2.0 { (x, c, 0.0) }
    else if h_prime >= 2.0 && h_prime < 3.0 { (0.0, c, x) }
    else if h_prime >= 3.0 && h_prime < 4.0 { (0.0, x, c) }
    else if h_prime >= 4.0 && h_prime < 5.0 { (x, 0.0, c) }
    else { (c, 0.0, x) };
    
    (r1 + m, g1 + m, b1 + m)
}
"""

if "fn hsl_to_rgb" not in content:
    content = content.replace("fn get_zone_for_key", hsl_fn + "\nfn get_zone_for_key")

# 2. Add anim state before set_draw_func
start_str = "    let selected_keys = Rc::new(RefCell::new(Vec::<String>::new()));"
new_start_str = """    let selected_keys = Rc::new(RefCell::new(Vec::<String>::new()));
    let anim_state = Rc::new(RefCell::new(("static".to_string(), 50.0, 0.0f64)));
    let anim_draw = anim_state.clone();
"""
content = content.replace(start_str, new_start_str)

# 3. Replace drawing area logic
draw_start_str = "        for k in keys_draw.iter() {"
draw_end_str = "            let (r, g, b) = parse_hex(hex);"

new_draw_str = """        let (anim_mode, anim_speed, anim_time) = anim_draw.borrow().clone();
        for k in keys_draw.iter() {
            let hex = cols.get(&k.name).map(|s| s.as_str()).unwrap_or("#141418");
            let (mut r, mut g, mut b) = parse_hex(hex);

            if anim_mode == "breathing" {
                let factor = (anim_time * anim_speed * 0.05).sin().abs() * 0.8 + 0.2;
                r *= factor; g *= factor; b *= factor;
            } else if anim_mode == "cycle" {
                let h = (anim_time * anim_speed * 2.0) % 360.0;
                let (nr, ng, nb) = hsl_to_rgb(h, 1.0, 0.5);
                r = nr; g = ng; b = nb;
            } else if anim_mode == "wave" {
                let offset = k.x / 10.0;
                let h = ((anim_time * anim_speed * 4.0) + offset * 8.0) % 360.0;
                let (nr, ng, nb) = hsl_to_rgb(h, 1.0, 0.5);
                r = nr; g = ng; b = nb;
            } else if anim_mode == "wave_rainbow" {
                let offset = k.x / 10.0;
                let h = ((anim_time * anim_speed * 4.0) - offset * 8.0) % 360.0;
                let (nr, ng, nb) = hsl_to_rgb(h, 1.0, 0.5);
                r = nr; g = ng; b = nb;
            }
"""

start_idx = content.find(draw_start_str)
end_idx = content.find(draw_end_str, start_idx) + len(draw_end_str)

content = content[:start_idx] + new_draw_str + content[end_idx:]


# 4. Replace apply_anim logic at the bottom
old_apply_anim = """    let da_anim = drawing_area.clone();
    let apply_anim: Rc<dyn Fn(&str, f64)> = Rc::new(move |_mode: &str, _speed: f64| {
        da_anim.queue_draw();
    });
    *apply_anim_rc.borrow_mut() = Some(apply_anim.clone());"""

new_apply_anim = """    let tick_id: Rc<RefCell<Option<gtk::TickCallbackId>>> = Rc::new(RefCell::new(None));
    let da_anim = drawing_area.clone();
    let anim_state_apply = anim_state.clone();
    
    let apply_anim: Rc<dyn Fn(&str, f64)> = Rc::new(move |mode: &str, speed: f64| {
        let mut ts = anim_state_apply.borrow_mut();
        ts.0 = mode.to_string();
        ts.1 = speed;
        drop(ts);
        
        let needs_anim = mode != "static" && mode != "per_key_custom";
        
        let mut curr_id = tick_id.borrow_mut();
        if !needs_anim && curr_id.is_some() {
            curr_id.take().unwrap().remove();
        } else if needs_anim && curr_id.is_none() {
            let anim_state_tick = anim_state_apply.clone();
            let da_tick = da_anim.clone();
            let id = da_anim.add_tick_callback(move |_, clock| {
                let frame_time = clock.frame_time() as f64 / 1_000_000.0;
                let mut ts = anim_state_tick.borrow_mut();
                ts.2 = frame_time;
                drop(ts);
                da_tick.queue_draw();
                glib::ControlFlow::Continue
            });
            *curr_id = Some(id);
        }
        da_anim.queue_draw();
    });
    *apply_anim_rc.borrow_mut() = Some(apply_anim.clone());"""

content = content.replace(old_apply_anim, new_apply_anim)

with open("src/omen-gui/src/keyboardrgb.rs", "w") as f:
    f.write(content)
