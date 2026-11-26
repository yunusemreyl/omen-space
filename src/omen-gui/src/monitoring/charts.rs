use gtk::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

use crate::i18n;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ChartMetric {
    Power,
    Temp,
    Load,
}

#[derive(Clone, Copy, Default)]
pub(super) struct HistorySample {
    pub(super) power: f64,
    pub(super) temp: f64,
    pub(super) load_pct: f64,
    pub(super) throttled: bool,
}

pub(super) fn build_metric_switcher(
    cpu_da: &gtk::DrawingArea,
    gpu_da: &gtk::DrawingArea,
    active_metric: &Rc<RefCell<ChartMetric>>,
) -> gtk::Box {
    let metric_pill = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .css_classes(["curve-pill"])
        .valign(gtk::Align::Center)
        .build();
    let btn_pwr = gtk::ToggleButton::builder()
        .label(i18n::t("mon_metric_power"))
        .css_classes(["curve-pill-btn"])
        .active(true)
        .build();
    let btn_tmp = gtk::ToggleButton::builder()
        .label(i18n::t("mon_metric_temp"))
        .css_classes(["curve-pill-btn"])
        .group(&btn_pwr)
        .build();
    let btn_load = gtk::ToggleButton::builder()
        .label(i18n::t("mon_metric_load"))
        .css_classes(["curve-pill-btn"])
        .group(&btn_pwr)
        .build();
    metric_pill.append(&btn_pwr);
    metric_pill.append(&btn_tmp);
    metric_pill.append(&btn_load);

    for (btn, target) in [
        (&btn_pwr, ChartMetric::Power),
        (&btn_tmp, ChartMetric::Temp),
        (&btn_load, ChartMetric::Load),
    ] {
        let m = active_metric.clone();
        let c_da = cpu_da.clone();
        let g_da = gpu_da.clone();
        btn.connect_toggled(move |b| {
            if b.is_active() {
                *m.borrow_mut() = target;
                c_da.queue_draw();
                g_da.queue_draw();
            }
        });
    }

    metric_pill
}

pub(super) fn attach_chart_draw_callback(
    drawing_area: &gtk::DrawingArea,
    hist: Rc<RefCell<Vec<HistorySample>>>,
    metric: Rc<RefCell<ChartMetric>>,
    max_watt: f64,
    color: (f64, f64, f64),
) {
    drawing_area.set_draw_func(move |widget, cr, w, h| {
        draw_telemetry_chart(
            widget,
            cr,
            w,
            h,
            &hist.borrow(),
            *metric.borrow(),
            max_watt,
            color,
        );
    });
}

pub(super) fn push_history_sample(
    history: &Rc<RefCell<Vec<HistorySample>>>,
    sample: HistorySample,
    drawing_area: &gtk::DrawingArea,
) {
    {
        let mut h = history.borrow_mut();
        h.remove(0);
        h.push(sample);
    }
    drawing_area.queue_draw();
}

// ── Theme-Adaptive Cairo Area Chart ─────────────────────────

#[allow(clippy::too_many_arguments)]
pub(super) fn draw_telemetry_chart(
    widget: &gtk::DrawingArea,
    cr: &gtk::cairo::Context,
    w: i32,
    h: i32,
    hist: &[HistorySample],
    metric: ChartMetric,
    max_watt: f64,
    (r, g, b): (f64, f64, f64),
) {
    let n = hist.len();
    if n < 2 || w <= 4 || h <= 4 {
        return;
    }

    let fg = widget.color();
    let (fr, fg_c, fb) = (fg.red() as f64, fg.green() as f64, fg.blue() as f64);

    let wf = w as f64;
    let hf = h as f64;
    let pad_t = 18.0;
    let pad_b = 6.0;
    let chart_h = (hf - pad_t - pad_b).max(1.0);
    let step = wf / (n - 1) as f64;

    let (max_val, unit, decimals) = match metric {
        ChartMetric::Power => (max_watt.max(10.0), "W", 1usize),
        ChartMetric::Temp => (105.0_f64, "°C", 0usize),
        ChartMetric::Load => (100.0_f64, "%", 0usize),
    };

    let sample_val = |s: &HistorySample| -> f64 {
        match metric {
            ChartMetric::Power => s.power,
            ChartMetric::Temp => s.temp,
            ChartMetric::Load => s.load_pct,
        }
    };

    // Transparent canvas
    cr.set_operator(gtk::cairo::Operator::Clear);
    let _ = cr.paint();
    cr.set_operator(gtk::cairo::Operator::Over);

    // Subtle horizontal guide lines (25%, 50%, 75%, 100%)
    cr.set_line_width(1.0);
    cr.set_source_rgba(fr, fg_c, fb, 0.07);
    for p in [0.0, 0.25, 0.5, 0.75, 1.0] {
        let y = (pad_t + chart_h * (1.0 - p)).round() + 0.5;
        cr.move_to(0.0, y);
        cr.line_to(wf, y);
        let _ = cr.stroke();
    }

    // Soft vertical gradient fill under the telemetry line
    cr.move_to(0.0, pad_t + chart_h);
    for (i, item) in hist.iter().enumerate() {
        let yv = (sample_val(item) / max_val).clamp(0.0, 1.0);
        cr.line_to(i as f64 * step, pad_t + chart_h * (1.0 - yv));
    }
    cr.line_to(wf, pad_t + chart_h);
    cr.close_path();

    let grad = gtk::cairo::LinearGradient::new(0.0, pad_t, 0.0, pad_t + chart_h);
    grad.add_color_stop_rgba(0.0, r, g, b, 0.24);
    grad.add_color_stop_rgba(1.0, r, g, b, 0.01);
    let _ = cr.set_source(&grad);
    let _ = cr.fill();

    // Primary stroke (gold segment if throttled)
    cr.set_line_width(2.0);
    cr.set_line_join(gtk::cairo::LineJoin::Round);
    cr.set_line_cap(gtk::cairo::LineCap::Round);
    for i in 1..n {
        let prev_y = (sample_val(&hist[i - 1]) / max_val).clamp(0.0, 1.0);
        let curr_y = (sample_val(&hist[i]) / max_val).clamp(0.0, 1.0);
        cr.move_to((i - 1) as f64 * step, pad_t + chart_h * (1.0 - prev_y));
        cr.line_to(i as f64 * step, pad_t + chart_h * (1.0 - curr_y));
        if hist[i].throttled {
            cr.set_source_rgba(0.96, 0.65, 0.14, 1.0);
        } else {
            cr.set_source_rgba(r, g, b, 1.0);
        }
        let _ = cr.stroke();
    }

    // Top-left scale & peak caption
    let peak = hist.iter().map(sample_val).fold(0.0_f64, f64::max);
    let max_lbl = format!("{:.0}{}", max_val, unit);
    let peak_lbl = if decimals == 1 {
        format!("{} {:.1}{}", i18n::t("mon_peak"), peak, unit)
    } else {
        format!("{} {:.0}{}", i18n::t("mon_peak"), peak, unit)
    };

    cr.select_font_face(
        "Sans",
        gtk::cairo::FontSlant::Normal,
        gtk::cairo::FontWeight::Normal,
    );
    cr.set_font_size(9.5);
    cr.set_source_rgba(fr, fg_c, fb, 0.45);
    cr.move_to(2.0, 11.0);
    let _ = cr.show_text(&max_lbl);

    if peak > 0.0 {
        let tw = cr
            .text_extents(&peak_lbl)
            .map(|e| e.width())
            .unwrap_or(48.0);
        cr.move_to((wf - tw - 4.0).max(46.0), 11.0);
        let _ = cr.show_text(&peak_lbl);
    }

    // Endpoint dot + clamped current value label
    let last_val = sample_val(&hist[n - 1]);
    let end_y = pad_t + chart_h * (1.0 - (last_val / max_val).clamp(0.0, 1.0));
    let val_str = if decimals == 1 {
        format!("{:.1}{}", last_val, unit)
    } else {
        format!("{:.0}{}", last_val, unit)
    };

    cr.select_font_face(
        "Sans",
        gtk::cairo::FontSlant::Normal,
        gtk::cairo::FontWeight::Bold,
    );
    cr.set_font_size(10.5);
    let vw = cr.text_extents(&val_str).map(|e| e.width()).unwrap_or(32.0);
    let label_y = (end_y - 7.0).clamp(pad_t + 12.0, pad_t + chart_h - 4.0);
    cr.set_source_rgba(fr, fg_c, fb, 0.90);
    cr.move_to((wf - vw - 10.0).max(2.0), label_y);
    let _ = cr.show_text(&val_str);

    // Endpoint indicator dot
    let dot_x = wf - 3.5;
    cr.set_source_rgba(r, g, b, 0.28);
    cr.arc(dot_x, end_y, 6.0, 0.0, std::f64::consts::TAU);
    let _ = cr.fill();

    if hist[n - 1].throttled {
        cr.set_source_rgba(0.96, 0.65, 0.14, 1.0);
    } else {
        cr.set_source_rgba(r, g, b, 1.0);
    }
    cr.arc(dot_x, end_y, 3.2, 0.0, std::f64::consts::TAU);
    let _ = cr.fill();
}
