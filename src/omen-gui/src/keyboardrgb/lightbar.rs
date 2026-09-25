use gtk::prelude::*;
use std::rc::Rc;
use std::cell::RefCell;
use crate::keyboardrgb::helpers::*;
pub fn build_interactive_lightbar(state_json_opt: &Option<serde_json::Value>) -> gtk::Box {
    let bar_card = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .css_classes(["os-card"])
        .spacing(12)
        .build();

    let title_row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .build();
    title_row.append(&gtk::Label::builder()
        .label(i18n::t("lightbar_title"))
        .css_classes(["chip-title"])
        .hexpand(true)
        .halign(gtk::Align::Start)
        .build());

    let sync_btn = gtk::Button::builder()
        .label(i18n::t("lightbar_sync_btn"))
        .css_classes(["ec-btn"])
        .build();
    title_row.append(&sync_btn);
    bar_card.append(&title_row);

    // 4 Segment Light Bar Strip
    let strip_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(6)
        .homogeneous(true)
        .margin_top(4)
        .margin_bottom(4)
        .build();

    let segment_names = [i18n::t("lb_seg_1"), i18n::t("lb_seg_2"), i18n::t("lb_seg_3"), i18n::t("lb_seg_4")];
    let default_colors = ["#E03454", "#0099EE", "#0099EE", "#E03454"]; // OMEN gradient
    let mut segment_buttons = Vec::new();

    let dyn_provider = Rc::new(gtk::CssProvider::new());
    if let Some(display) = gtk::gdk::Display::default() {
        gtk::style_context_add_provider_for_display(&display, &*dyn_provider, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);
    }
    let mut initial_colors = default_colors.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    if let Some(state) = state_json_opt {
        if let Some(zones) = state.get("zones").and_then(|z| z.as_object()) {
            for i in 0..4 {
                let key = format!("{}", i + 4); // Lightbar zones are 4, 5, 6, 7
                if let Some(hex) = zones.get(&key).and_then(|h| h.as_str()) {
                    initial_colors[i] = hex.to_string();
                }
            }
        }
    }
    let colors_map = Rc::new(RefCell::new(initial_colors));

    for (i, name) in segment_names.iter().enumerate() {
        let seg_btn = gtk::Button::builder()
            .label(*name)
            .height_request(46)
            .build();
        
        seg_btn.set_widget_name(&format!("lightbar_seg_{}", i));
        
        let dyn_prov_inner = dyn_provider.clone();
        let colors_inner = colors_map.clone();
        
        seg_btn.connect_clicked(move |btn_ref| {
            let colors_local = colors_inner.clone();
            let dyn_local = dyn_prov_inner.clone();
            
            show_color_picker_popover(btn_ref, Rc::new(move |hex| {
                colors_local.borrow_mut()[i] = hex.clone();
                
                let mut css_str = String::new();
                for (idx, c) in colors_local.borrow().iter().enumerate() {
                    css_str.push_str(&format!(
                        "#lightbar_seg_{} {{ background: {}; background-image: none; color: #fff; border-radius: 6px; font-size: 11px; font-weight: bold; border: 1px solid rgba(255,255,255,0.25); box-shadow: 0 0 14px {}60; transition: all 0.25s ease; }}\n",
                        idx, c, c
                    ));
                }
                dyn_local.load_from_string(&css_str);
                
                // Assuming lightbar uses zones 4, 5, 6, 7
                let zone_id = 4 + i as i32;
                crate::daemon_client::set_color_sync(zone_id, hex.clone());
            }));
        });

        strip_box.append(&seg_btn);
        segment_buttons.push(seg_btn);
    }
    bar_card.append(&strip_box);

    let dyn_prov_inner_sync = dyn_provider.clone();
    let colors_inner_sync = colors_map.clone();
    
    sync_btn.connect_clicked(move |btn_ref| {
        let colors_local = colors_inner_sync.clone();
        let dyn_local = dyn_prov_inner_sync.clone();
        
        show_color_picker_popover(btn_ref, Rc::new(move |hex| {
            let mut css_str = String::new();
            for i in 0..4 {
                colors_local.borrow_mut()[i] = hex.clone();
                css_str.push_str(&format!(
                    "#lightbar_seg_{} {{ background: {}; background-image: none; color: #fff; border-radius: 6px; font-size: 11px; font-weight: bold; border: 1px solid rgba(255,255,255,0.25); box-shadow: 0 0 14px {}60; transition: all 0.25s ease; }}\n",
                    i, hex, hex
                ));
                crate::daemon_client::set_color_sync((4 + i) as i32, hex.clone());
            }
            dyn_local.load_from_string(&css_str);
        }));
    });

    // Initialize initial colors
    let mut init_css = String::new();
    for (idx, c) in colors_map.borrow().iter().enumerate() {
        init_css.push_str(&format!(
            "#lightbar_seg_{} {{ background: {}; background-image: none; color: #fff; border-radius: 6px; font-size: 11px; font-weight: bold; border: 1px solid rgba(255,255,255,0.15); box-shadow: 0 0 10px {}40; transition: all 0.25s ease; }}\n",
            idx, c, c
        ));
    }
    dyn_provider.load_from_string(&init_css);

    bar_card
}

use crate::i18n;


