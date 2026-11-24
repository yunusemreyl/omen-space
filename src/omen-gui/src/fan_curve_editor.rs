use gtk::prelude::*;
use std::rc::Rc;
use std::cell::RefCell;
use crate::i18n;

pub fn show_fan_curve_editor(
    parent: &impl IsA<gtk::Window>,
    preset_name: &str,
    points: Vec<(f64, f64)>,
    on_save: impl Fn(Vec<(f64, f64)>) + 'static,
    on_delete: impl Fn() + 'static,
) {
    let dialog = gtk::Window::builder()
        .title(i18n::t("custom_curve_title"))
        .transient_for(parent)
        .modal(true)
        .destroy_with_parent(true)
        .default_width(360)
        .default_height(420)
        .css_classes(["os-card"])
        .build();

    let vbox = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .margin_top(12)
        .margin_bottom(12)
        .margin_start(12)
        .margin_end(12)
        .build();

    vbox.append(&gtk::Label::builder()
        .label(i18n::t("editing_preset").replace("{}", preset_name))
        .css_classes(["os-section-header"])
        .halign(gtk::Align::Start)
        .build());

    vbox.append(&gtk::Label::builder()
        .label(i18n::t("custom_curve_hint"))
        .css_classes(["os-section-desc"])
        .halign(gtk::Align::Start)
        .build());

    let pts = Rc::new(RefCell::new(points));

    let da = gtk::DrawingArea::builder()
        .width_request(320)
        .height_request(320)
        .hexpand(false)
        .vexpand(false)
        .halign(gtk::Align::Center)
        .build();

    let d_pts = pts.clone();
    da.set_draw_func(move |_, cr, w, h| {
        let pts = d_pts.borrow();
        let (r, g, b) = (1.0, 0.25, 0.4);

        let pad = 36.0_f64;
        let aw  = w as f64 - 2.0 * pad;
        let ah  = h as f64 - 2.0 * pad;

        let to_canvas = |temp: f64, speed: f64| -> (f64, f64) {
            (pad + (temp - 40.0) / 60.0 * aw,
             pad + (1.0 - speed / 100.0) * ah)
        };

        cr.set_operator(gtk::cairo::Operator::Clear);
        cr.paint().expect("Invalid cairo surface");
        cr.set_operator(gtk::cairo::Operator::Over);

        cr.set_line_width(0.5);
        cr.set_source_rgba(0.5, 0.5, 0.5, 0.15);
        for i in 1..=4 {
            let x = pad + aw * i as f64 / 4.0;
            cr.move_to(x, pad); cr.line_to(x, pad + ah); let _ = cr.stroke();
            let y = pad + ah * i as f64 / 4.0;
            cr.move_to(pad, y); cr.line_to(pad + aw, y); let _ = cr.stroke();
        }

        cr.set_source_rgba(0.5, 0.5, 0.5, 0.3);
        cr.set_line_width(1.0);
        cr.rectangle(pad, pad, aw, ah);
        let _ = cr.stroke();

        cr.select_font_face("Sans", gtk::cairo::FontSlant::Normal, gtk::cairo::FontWeight::Normal);
        cr.set_font_size(9.0);
        cr.set_source_rgba(0.5, 0.5, 0.5, 1.0);
        for (i, temp) in [40, 55, 70, 85, 100].iter().enumerate() {
            let x = pad + aw * i as f64 / 4.0;
            cr.move_to(x - 8.0, pad + ah + 14.0);
            let _ = cr.show_text(&format!("{}°C", temp));
        }
        for (i, pct) in ["100%", " 75%", " 50%", " 25%", "  0%"].iter().enumerate() {
            let y = pad + ah * i as f64 / 4.0;
            cr.move_to(2.0, y + 4.0);
            let _ = cr.show_text(pct);
        }

        cr.set_font_size(8.5);
        cr.save().unwrap();
        cr.translate(10.0, pad + ah / 2.0);
        cr.rotate(-std::f64::consts::FRAC_PI_2);
        cr.move_to(-22.0, 0.0);
        let _ = cr.show_text("FAN %");
        cr.restore().unwrap();

        cr.move_to(pad + aw / 2.0 - 22.0, pad + ah + 26.0);
        let _ = cr.show_text("TEMP °C");

        if pts.is_empty() { return; }

        let (x0, y0) = to_canvas(pts[0].0, pts[0].1);
        cr.move_to(x0, pad + ah);
        cr.line_to(x0, y0);
        for p in pts.iter().skip(1) {
            let (x, y) = to_canvas(p.0, p.1);
            cr.line_to(x, y);
        }
        if let Some(last) = pts.last() {
            let (xl, _) = to_canvas(last.0, last.1);
            cr.line_to(xl, pad + ah);
        }
        cr.close_path();
        cr.set_source_rgba(r, g, b, 0.10);
        let _ = cr.fill();

        cr.set_source_rgba(r, g, b, 1.0);
        cr.set_line_width(2.5);
        let (x0, y0) = to_canvas(pts[0].0, pts[0].1);
        cr.move_to(x0, y0);
        for p in pts.iter().skip(1) {
            let (x, y) = to_canvas(p.0, p.1);
            cr.line_to(x, y);
        }
        let _ = cr.stroke();
        
        cr.set_source_rgba(1.0, 1.0, 1.0, 1.0);
        for p in pts.iter() {
            let (x, y) = to_canvas(p.0, p.1);
            cr.arc(x, y, 4.0, 0.0, 2.0 * std::f64::consts::PI);
            let _ = cr.fill();
        }
    });

    // Simple interaction logic using EventControllerMotion and GestureClick
    let motion = gtk::EventControllerMotion::new();
    let da_m = da.clone();
    let pts_m = pts.clone();
    let hover_idx = Rc::new(RefCell::new(None));
    let hover_clone = hover_idx.clone();
    
    motion.connect_motion(move |_, x, y| {
        let pad = 36.0_f64;
        let aw = da_m.width() as f64 - 2.0 * pad;
        let ah = da_m.height() as f64 - 2.0 * pad;
        let temp = 40.0 + (x - pad) / aw * 60.0;
        let speed = 100.0 - (y - pad) / ah * 100.0;
        
        let mut closest = None;
        let mut min_dist = 9999.0;
        for (i, p) in pts_m.borrow().iter().enumerate() {
            let dx = p.0 - temp;
            let dy = p.1 - speed;
            let dist = dx*dx + dy*dy;
            if dist < 400.0 && dist < min_dist {
                min_dist = dist;
                closest = Some(i);
            }
        }
        *hover_clone.borrow_mut() = closest;
    });
    da.add_controller(motion);

    let drag = gtk::GestureDrag::new();
    let pts_d = pts.clone();
    let da_d = da.clone();
    let h_d = hover_idx.clone();
    drag.connect_drag_update(move |gesture, offset_x, offset_y| {
        if let Some(idx) = *h_d.borrow() {
            if let Some((start_x, start_y)) = gesture.start_point() {
                let x = start_x + offset_x;
                let y = start_y + offset_y;
                let pad = 36.0_f64;
                let aw = da_d.width() as f64 - 2.0 * pad;
                let ah = da_d.height() as f64 - 2.0 * pad;
                
                let mut temp = (40.0 + (x - pad) / aw * 60.0).clamp(40.0, 100.0);
                let speed = (100.0 - (y - pad) / ah * 100.0).clamp(0.0, 100.0);
                
                let mut p = pts_d.borrow_mut();
                if idx > 0
                    && temp <= p[idx-1].0 { temp = p[idx-1].0 + 1.0; }
                if idx < p.len() - 1
                    && temp >= p[idx+1].0 { temp = p[idx+1].0 - 1.0; }
                
                p[idx] = (temp, speed);
                da_d.queue_draw();
            }
        }
    });
    da.add_controller(drag);

    vbox.append(&da);

    let hbox = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .halign(gtk::Align::End)
        .build();

    let save_btn = gtk::Button::builder()
        .label(i18n::t("save"))
        .css_classes(["suggested-action"])
        .build();
    let pts_save = pts.clone();
    let d2 = dialog.clone();
    save_btn.connect_clicked(move |_| {
        on_save(pts_save.borrow().clone());
        d2.destroy();
    });

    let delete_btn = gtk::Button::builder()
        .label(i18n::t("delete"))
        .css_classes(["destructive-action"])
        .build();
    let d3 = dialog.clone();
    delete_btn.connect_clicked(move |_| {
        on_delete();
        d3.destroy();
    });

    hbox.append(&delete_btn);
    hbox.append(&save_btn);
    vbox.append(&hbox);

    dialog.set_child(Some(&vbox));
    dialog.present();
}
