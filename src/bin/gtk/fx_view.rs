//! Theme extras on RyM GTK, drawn with cairo: the cover as a spinning record, Liquid Aurora
//! behind the playing song's page, and the two needle meters. The app sets [`Live`] each tick;
//! the drawings redraw only while something moves (nothing is drawn while paused).

use std::cell::{Cell, RefCell};
use std::f64::consts::TAU;
use std::rc::Rc;

use gtk::prelude::*;
use gtk::{cairo, gdk, glib};

/// What the drawings follow, set by the app.
pub struct Live {
    pub playing: Cell<bool>,
    /// The record's turn, degrees a second (33⅓ rpm = 200, times the speed).
    pub spin_rate: Cell<f64>,
    /// The bass now, 0 to 1.
    pub energy: Cell<f64>,
    /// Left and right, 0 to 1.
    pub vu: Cell<(f64, f64)>,
    /// The cover's four main colors.
    pub lights: RefCell<[(f64, f64, f64); 4]>,
    pub dark: Cell<bool>,
    /// The page's background (the record's hole shows it).
    pub bg: Cell<(f64, f64, f64)>,
}

impl Default for Live {
    fn default() -> Self {
        Live {
            playing: Cell::new(false),
            spin_rate: Cell::new(200.0),
            energy: Cell::new(0.0),
            vu: Cell::new((0.0, 0.0)),
            lights: RefCell::new([(0.92, 0.5, 0.29), (0.56, 0.27, 0.68), (0.16, 0.5, 0.73), (0.09, 0.63, 0.52)]),
            dark: Cell::new(true),
            bg: Cell::new((0.07, 0.07, 0.08)),
        }
    }
}

/// A texture as a cairo picture (GDK's default download is cairo's ARGB32 order).
fn surface_of(texture: &gdk::Texture) -> Option<cairo::ImageSurface> {
    let mut s = cairo::ImageSurface::create(cairo::Format::ARgb32, texture.width(), texture.height()).ok()?;
    let stride = s.stride() as usize;
    {
        let mut data = s.data().ok()?;
        texture.download(&mut data, stride);
    }
    s.mark_dirty();
    Some(s)
}

/// The cover's four main colors (for Liquid Aurora).
pub fn cover_lights(texture: &gdk::Texture) -> Option<[(f64, f64, f64); 4]> {
    let (w, h) = (texture.width() as usize, texture.height() as usize);
    let mut px = vec![0u8; w * h * 4];
    texture.download(&mut px, w * 4);
    // BGRA as RGBA.
    for p in px.as_chunks_mut::<4>().0 {
        p.swap(0, 2);
    }
    let c = rymplayer::themes::cover_colors(&px, 4);
    let f = |c: (u8, u8, u8)| (c.0 as f64 / 255.0, c.1 as f64 / 255.0, c.2 as f64 / 255.0);
    match c[..] {
        [a, b, cc, d] => Some([f(a), f(b), f(cc), f(d)]),
        _ => None,
    }
}

thread_local! {
    /// Animations may move (the window in front and shown, no battery saver), and their frames
    /// a second (0: as fast as the screen). Set by the app.
    static ECO: Cell<(bool, u32)> = const { Cell::new((true, 60)) };
}

/// The app's say on the animations: whether they may move, and how many frames a second.
pub fn set_eco(allowed: bool, fps: u32) {
    ECO.with(|e| e.set((allowed, fps)));
}

/// Redraws `area` while `moving()` says so and the app allows it, at most at the chosen frames a
/// second; `step` gets the seconds since the last drawn frame.
fn animate(area: &gtk::DrawingArea, moving: impl Fn() -> bool + 'static, step: impl Fn(f64) + 'static) {
    let last = Cell::new(0i64);
    let waited = Cell::new(0.0f64);
    area.add_tick_callback(move |a, clock| {
        let now = clock.frame_time();
        let before = last.replace(now);
        let (allowed, fps) = ECO.with(|e| e.get());
        if before > 0 && allowed && moving() {
            waited.set(waited.get() + (now - before) as f64 / 1e6);
            if fps == 0 || waited.get() >= 1.0 / fps as f64 {
                step(waited.replace(0.0).min(0.1));
                a.queue_draw();
            }
        } else {
            waited.set(0.0);
        }
        glib::ControlFlow::Continue
    });
}

/// The cover as a record: grooves, the cover as its label turning while it plays.
pub fn vinyl(art: &gtk::Picture, live: Rc<Live>) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::new();
    area.set_hexpand(true);
    area.set_vexpand(true);
    let angle = Rc::new(Cell::new(0.0f64));
    {
        let (angle, l1, l2) = (angle.clone(), live.clone(), live.clone());
        animate(&area, move || l1.playing.get(), move |dt| angle.set((angle.get() + dt * l2.spin_rate.get()) % 360.0));
    }
    let label: RefCell<Option<(gdk::Texture, cairo::ImageSurface)>> = RefCell::new(None);
    let art = art.clone();
    area.set_draw_func(move |_, cr, w, h| {
        let r = w.min(h) as f64 / 2.0;
        let (cx, cy) = (w as f64 / 2.0, h as f64 / 2.0);
        cr.arc(cx, cy, r, 0.0, TAU);
        cr.set_source_rgb(0.047, 0.047, 0.05);
        let _ = cr.fill();
        cr.set_line_width(1.0);
        for i in 0..10 {
            cr.set_source_rgba(1.0, 1.0, 1.0, if i % 3 == 0 { 0.13 } else { 0.055 });
            cr.arc(cx, cy, r * (0.97 - i as f64 * 0.052), 0.0, TAU);
            let _ = cr.stroke();
        }
        let texture = art.paintable().and_then(|p| p.downcast::<gdk::Texture>().ok());
        if let Some(t) = texture {
            let mut cache = label.borrow_mut();
            if cache.as_ref().is_none_or(|(old, _)| *old != t) {
                *cache = surface_of(&t).map(|s| (t.clone(), s));
            }
            if let Some((_, s)) = cache.as_ref() {
                let lr = r * 0.4;
                let _ = cr.save();
                cr.arc(cx, cy, lr, 0.0, TAU);
                cr.clip();
                cr.translate(cx, cy);
                cr.rotate(angle.get().to_radians());
                let k = lr * 2.0 / s.width().min(s.height()).max(1) as f64;
                cr.scale(k, k);
                let _ = cr.set_source_surface(s, -s.width() as f64 / 2.0, -s.height() as f64 / 2.0);
                let _ = cr.paint();
                let _ = cr.restore();
            }
        }
        let (br, bg, bb) = live.bg.get();
        cr.set_source_rgb(br, bg, bb);
        cr.arc(cx, cy, r * 0.0175, 0.0, TAU);
        let _ = cr.fill();
        // The light on it stays still while it turns.
        let g = cairo::LinearGradient::new(cx - r, cy - r, cx + r, cy + r);
        g.add_color_stop_rgba(0.28, 1.0, 1.0, 1.0, 0.0);
        g.add_color_stop_rgba(0.46, 1.0, 1.0, 1.0, 0.09);
        g.add_color_stop_rgba(0.55, 1.0, 1.0, 1.0, 0.0);
        g.add_color_stop_rgba(0.82, 1.0, 1.0, 1.0, 0.05);
        g.add_color_stop_rgba(0.9, 1.0, 1.0, 1.0, 0.0);
        cr.arc(cx, cy, r, 0.0, TAU);
        let _ = cr.set_source(&g);
        let _ = cr.fill();
    });
    area
}

/// Liquid Aurora: the cover's four colors as soft lights, bigger and brighter with the bass.
pub fn aurora(live: Rc<Live>) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::new();
    area.set_hexpand(true);
    area.set_vexpand(true);
    area.set_can_target(false);
    let (t, e) = (Rc::new(Cell::new(0.0f64)), Rc::new(Cell::new(0.0f64)));
    {
        let (t, e, l1, l2) = (t.clone(), e.clone(), live.clone(), live.clone());
        let e2 = e.clone();
        animate(
            &area,
            move || l1.playing.get() || e2.get() > 0.002,
            move |dt| {
                if l2.playing.get() {
                    t.set(t.get() + dt / 6.0);
                }
                let target = if l2.playing.get() { l2.energy.get() } else { 0.0 };
                e.set(e.get() + (target - e.get()) * (dt * 8.0).min(1.0));
            },
        );
    }
    area.set_draw_func(move |_, cr, w, h| {
        let (w, h) = (w as f64, h as f64);
        let lights = *live.lights.borrow();
        let alpha = if live.dark.get() { 0.5 } else { 0.4 } * (0.55 + e.get() * 0.45);
        for (i, (bx, by, s)) in [(0.18, 0.22, 0.8), (0.84, 0.2, 0.7), (0.28, 0.86, 0.75), (0.82, 0.8, 0.65)].into_iter().enumerate() {
            let size = w.min(h) * s * (1.0 + e.get() * 0.4);
            let x = w * (bx + ((t.get() + i as f64 * 1.7).sin() * 0.05));
            let y = h * (by + ((t.get() * 0.8 + i as f64 * 2.3).cos() * 0.05));
            let (r, g, b) = lights[i];
            let grad = cairo::RadialGradient::new(x, y, 0.0, x, y, size * 0.5 * 0.7);
            grad.add_color_stop_rgba(0.0, r, g, b, alpha);
            grad.add_color_stop_rgba(1.0, r, g, b, 0.0);
            let _ = cr.set_source(&grad);
            cr.rectangle(x - size, y - size, size * 2.0, size * 2.0);
            let _ = cr.fill();
        }
    });
    area
}

/// Two needle meters, left and right (Hi-Fi Rack).
pub fn meters(live: Rc<Live>) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::new();
    area.set_content_height(72);
    area.set_hexpand(true);
    let needles = Rc::new(Cell::new((0.0f64, 0.0f64)));
    {
        let (n, l1, l2) = (needles.clone(), live.clone(), live.clone());
        let n2 = needles.clone();
        animate(
            &area,
            move || l1.playing.get() || n2.get().0 > 0.002 || n2.get().1 > 0.002,
            move |dt| {
                let (tl, tr) = if l2.playing.get() { l2.vu.get() } else { (0.0, 0.0) };
                let (l, r) = n.get();
                let k = (dt * 11.0).min(1.0);
                n.set((l + (tl - l) * k, r + (tr - r) * k));
            },
        );
    }
    area.set_draw_func(move |_, cr, w, h| {
        let gap = 14.0;
        let mw = (w as f64 - gap) / 2.0;
        let (l, r) = needles.get();
        for (i, (level, name)) in [(l, "L"), (r, "R")].into_iter().enumerate() {
            let x0 = i as f64 * (mw + gap);
            let hh = h as f64;
            let face = cairo::LinearGradient::new(0.0, 0.0, 0.0, hh);
            face.add_color_stop_rgb(0.0, 0.957, 0.925, 0.831);
            face.add_color_stop_rgb(1.0, 0.867, 0.816, 0.678);
            cr.rectangle(x0, 0.0, mw, hh);
            let _ = cr.set_source(&face);
            let _ = cr.fill_preserve();
            cr.set_source_rgb(0.05, 0.05, 0.05);
            cr.set_line_width(2.0);
            let _ = cr.stroke();
            let (px, py) = (x0 + mw / 2.0, hh * 0.96);
            let reach = hh * 0.8;
            for k in 0..11 {
                let a = (-50.0 + k as f64 * 10.0f64).to_radians();
                let len = if k >= 8 { 11.0 } else { 7.0 };
                if k >= 8 {
                    cr.set_source_rgb(0.75, 0.22, 0.17);
                } else {
                    cr.set_source_rgb(0.17, 0.17, 0.17);
                }
                cr.set_line_width(2.0);
                cr.move_to(px + a.sin() * reach, py - a.cos() * reach);
                cr.line_to(px + a.sin() * (reach - len), py - a.cos() * (reach - len));
                let _ = cr.stroke();
            }
            cr.set_source_rgb(0.17, 0.17, 0.17);
            cr.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Bold);
            cr.set_font_size(12.0);
            cr.move_to(px - 9.0, hh * 0.5);
            let _ = cr.show_text("VU");
            cr.set_font_size(11.0);
            cr.move_to(x0 + 8.0, hh - 6.0);
            let _ = cr.show_text(name);
            let a = (-50.0 + level * 100.0).to_radians();
            cr.set_source_rgb(0.086, 0.086, 0.086);
            cr.set_line_width(2.0);
            cr.move_to(px, py);
            cr.line_to(px + a.sin() * hh * 0.86, py - a.cos() * hh * 0.86);
            let _ = cr.stroke();
            cr.arc(px, py, 5.0, 0.0, TAU);
            let _ = cr.fill();
        }
    });
    area
}
