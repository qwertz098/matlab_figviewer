#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod mat;

use eframe::egui::{self, Color32};
use egui_plot::{Line, Plot, PlotPoints};

struct Sig {
    label: String,
    color: Color32,
    visible: bool,
    factor: f64,
    pts: Vec<[f64; 2]>,
}

struct Tab {
    name: String,
    sigs: Vec<Sig>,
    filter: String,
    refit: bool,
}

#[derive(Default)]
struct App {
    tabs: Vec<Tab>,
    sel: usize,
    file: Option<String>,
    error: Option<String>,
    cursor_t: Option<f64>,
    hit_sig: Option<usize>,
}

fn palette(i: usize) -> Color32 {
    let h = (i as f32 * 0.381_966) % 1.0;
    let (s, v) = if i % 2 == 0 { (0.90, 0.72) } else { (0.80, 0.50) };
    egui::ecolor::Hsva::new(h, s, v, 1.0).into()
}

fn fmt(v: f64) -> String {
    let a = v.abs();
    if v != 0.0 && !(1e-3..1e6).contains(&a) {
        format!("{v:.4e}")
    } else {
        let t = format!("{v:.4}");
        t.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

/// Wert des Abtastpunkts, der der Zeit `t` am nächsten liegt.
fn value_at(pts: &[[f64; 2]], t: f64) -> Option<f64> {
    if pts.is_empty() {
        return None;
    }
    let i = pts.partition_point(|q| q[0] < t);
    let j = if i == 0 {
        0
    } else if i >= pts.len() || (t - pts[i - 1][0]) <= (pts[i][0] - t) {
        i - 1
    } else {
        i
    };
    Some(pts[j][1])
}

fn load(path: &str) -> anyhow::Result<Vec<Tab>> {
    let fig = mat::load_figure(path)?;
    let mut lines = Vec::new();
    mat::collect_lines(&fig, &mut lines);
    if lines.is_empty() {
        anyhow::bail!("Die Figure enthält keine Linien");
    }
    // Gruppe = Text vor dem ersten Punkt ("50Hz.speedcpu" -> Tab "50Hz").
    let mut tabs: Vec<Tab> = Vec::new();
    for l in lines {
        let (group, label) = match l.name.split_once('.') {
            Some((g, rest)) => (g.to_string(), rest.to_string()),
            None => ("Signale".to_string(), l.name.clone()),
        };
        let idx = match tabs.iter().position(|t| t.name == group) {
            Some(i) => i,
            None => {
                tabs.push(Tab { name: group, sigs: vec![], filter: String::new(), refit: true });
                tabs.len() - 1
            }
        };
        let tab = &mut tabs[idx];
        let pts = l.x.iter().zip(&l.y).map(|(&x, &y)| [x, y]).collect();
        tab.sigs.push(Sig {
            label,
            color: palette(tab.sigs.len()),
            visible: true,
            factor: 1.0,
            pts,
        });
    }
    let rank = |n: &str| match n {
        "50Hz" => 0,
        "10kHz" => 1,
        _ => 2,
    };
    tabs.sort_by_key(|t| rank(&t.name));
    Ok(tabs)
}

impl App {
    fn open(&mut self, path: &str) {
        match load(path) {
            Ok(tabs) => {
                self.tabs = tabs;
                self.sel = 0;
                self.file = Some(path.to_string());
                self.error = None;
            }
            Err(e) => self.error = Some(format!("{path}\n{e}")),
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let dropped = ctx.input(|i| i.raw.dropped_files.clone());
        if let Some(p) = dropped.iter().next().map(|f| f.path()) {
            self.open(&p.to_string_lossy());
        }

        egui::Panel::top("top").show(ui, |ui| {
            ui.horizontal(|ui| {
                for (i, t) in self.tabs.iter().enumerate() {
                    if ui.selectable_label(self.sel == i, &t.name).clicked() {
                        self.sel = i;
                    }
                }
                ui.separator();
                match &self.file {
                    Some(f) => ui.label(f),
                    None => ui.label("Figure-Datei (.fig) ins Fenster ziehen"),
                };
            });
        });

        if let Some(e) = &self.error {
            egui::Panel::bottom("err").show(ui, |ui| {
                ui.colored_label(Color32::LIGHT_RED, e);
            });
        }

        if self.tabs.is_empty() {
            egui::CentralPanel::default().show(ui, |ui| {
                ui.centered_and_justified(|ui| {
                    ui.heading("MATLAB-.fig-Datei hierher ziehen");
                });
            });
            return;
        }

        let sel = self.sel.min(self.tabs.len() - 1);
        let tab = &mut self.tabs[sel];
        let mut hovered: Option<usize> = None;
        let cursor = self.cursor_t;
        let hit_prev = self.hit_sig;

        egui::Panel::right("legend").default_size(430.0).show(ui, |ui| {
            ui.horizontal(|ui| {
                if ui.button("Alle an").clicked() {
                    tab.sigs.iter_mut().for_each(|s| s.visible = true);
                    tab.refit = true;
                }
                if ui.button("Alle aus").clicked() {
                    tab.sigs.iter_mut().for_each(|s| s.visible = false);
                }
                if ui.button("Faktoren = 1").clicked() {
                    tab.sigs.iter_mut().for_each(|s| s.factor = 1.0);
                    tab.refit = true;
                }
            });
            ui.horizontal(|ui| {
                ui.label("Filter:");
                ui.text_edit_singleline(&mut tab.filter);
            });
            ui.label("Faktor (Doppelklick = Wert tippen, z. B. 1e-3)");
            match cursor {
                Some(t) => ui.monospace(format!("t = {} s", fmt(t))),
                None => ui.weak("Maus ins Diagramm: Werte zum Zeitpunkt"),
            };
            ui.separator();
            let filt = tab.filter.to_lowercase();
            egui::ScrollArea::vertical().show(ui, |ui| {
                for (i, s) in tab.sigs.iter_mut().enumerate() {
                    if !filt.is_empty() && !s.label.to_lowercase().contains(&filt) {
                        continue;
                    }
                    let r = ui.horizontal(|ui| {
                        let d = ui.add(
                            egui::DragValue::new(&mut s.factor)
                                .speed(0.01)
                                .max_decimals(9)
                                .prefix("× "),
                        );
                        if d.changed() {
                            tab.refit = true;
                        }
                        let bold = hit_prev == Some(i);
                        let mut text = egui::RichText::new(&s.label).color(if s.visible {
                            s.color
                        } else {
                            Color32::GRAY
                        });
                        if bold {
                            text = text.strong().size(15.0);
                        }
                        ui.checkbox(&mut s.visible, text);
                        if let (Some(t), true) = (cursor, s.visible) {
                            if let Some(v) = value_at(&s.pts, t) {
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    let mut t = egui::RichText::new(fmt(v * s.factor)).monospace();
                                    if bold {
                                        t = t.strong().size(15.0);
                                    }
                                    ui.label(t);
                                });
                            }
                        }
                    });
                    if r.response.hovered() {
                        hovered = Some(i);
                    }
                }
            });
        });

        egui::CentralPanel::default().show(ui, |ui| {
            let mut fit = if std::mem::take(&mut tab.refit) { Some((true, true)) } else { None };
            ui.horizontal(|ui| {
                if ui.button("Ansicht zurücksetzen").clicked() {
                    fit = Some((true, true));
                }
                if ui.button("Zeitachse einpassen").clicked() {
                    fit = Some((true, false));
                }
                if ui.button("Y-Achse einpassen").clicked() {
                    fit = Some((false, true));
                }
                ui.weak("Zoom: Rechteck mit linker Maustaste · Verschieben: rechte Maustaste · Mausrad zoomt · Doppelklick = einpassen");
            });
            let plot = Plot::new(format!("plot_{}", tab.name))
                .x_axis_label("t [s]")
                .allow_double_click_reset(false)
                .allow_boxed_zoom(true)
                .boxed_zoom_pointer_button(egui::PointerButton::Primary)
                .pan_pointer_button(egui::PointerButton::Secondary);
            let mut hit_out: Option<(usize, [f64; 2], egui::Pos2)> = None;
            let mut cursor_out: Option<f64> = None;
            let plot = plot.show_x(false).show_y(false);
            let resp = plot.show(ui, |p| {
                let (mut x0, mut x1, mut y0, mut y1) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
                // Nächster Datenpunkt unter dem Mauszeiger (in Bildschirmpixeln)
                let mut hit: Option<(usize, [f64; 2], egui::Pos2)> = None;
                if let Some(ptr) = p.response().hover_pos() {
                    let mut best = 15.0_f32 * 15.0;
                    for (i, s) in tab.sigs.iter().enumerate() {
                        if !s.visible {
                            continue;
                        }
                        for &[x, y] in &s.pts {
                            let pos = p.transform().position_from_point(&egui_plot::PlotPoint::new(x, y * s.factor));
                            let d = pos.distance_sq(ptr);
                            if d < best {
                                best = d;
                                hit = Some((i, [x, y], pos));
                            }
                        }
                    }
                }
                hit_out = hit;
                cursor_out = match hit {
                    Some((_, [x, _], _)) => Some(x),
                    None => p.response().hover_pos().and(p.pointer_coordinate()).map(|c| c.x),
                };
                for (i, s) in tab.sigs.iter().enumerate() {
                    if !s.visible {
                        continue;
                    }
                    let f = s.factor;
                    let pts: Vec<[f64; 2]> = s.pts.iter().map(|&[x, y]| [x, y * f]).collect();
                    for q in &pts {
                        if q[0].is_finite() && q[1].is_finite() {
                            x0 = x0.min(q[0]);
                            x1 = x1.max(q[0]);
                            y0 = y0.min(q[1]);
                            y1 = y1.max(q[1]);
                        }
                    }
                    let bold = hovered == Some(i) || hit.is_some_and(|h| h.0 == i);
                    let w = if bold { 3.5 } else { 1.3 };
                    p.line(Line::new(&s.label, PlotPoints::from(pts)).color(s.color).width(w));
                }
                if let Some((i, [x, y], _)) = hit {
                    let s = &tab.sigs[i];
                    p.points(
                        egui_plot::Points::new(&s.label, vec![[x, y * s.factor]])
                            .radius(5.0)
                            .color(s.color),
                    );
                }
                if p.response().double_clicked() {
                    fit = Some((true, true));
                }
                if let Some((fx, fy)) = fit {
                    if x0 <= x1 && y0 <= y1 {
                        let cur = p.plot_bounds();
                        let pad = |lo: f64, hi: f64| {
                            let d = if hi > lo { (hi - lo) * 0.05 } else { 1.0 };
                            (lo - d, hi + d)
                        };
                        let (nx0, nx1) = if fx { pad(x0, x1) } else { (cur.min()[0], cur.max()[0]) };
                        let (ny0, ny1) = if fy { pad(y0, y1) } else { (cur.min()[1], cur.max()[1]) };
                        p.set_plot_bounds(egui_plot::PlotBounds::from_min_max([nx0, ny0], [nx1, ny1]));
                    }
                }
            });
            let _ = resp;
            let hit_now = hit_out.map(|h| h.0);
            if hit_now != self.hit_sig {
                self.hit_sig = hit_now;
                ui.ctx().request_repaint();
            }
            if cursor_out != self.cursor_t {
                self.cursor_t = cursor_out;
                ui.ctx().request_repaint();
            }
            if let Some((i, [x, y], pos)) = hit_out {
                let s = &tab.sigs[i];
                let mut lines = vec![
                    s.label.clone(),
                    format!("t = {} s", fmt(x)),
                    format!("Wert = {}", fmt(y * s.factor)),
                ];
                if s.factor != 1.0 {
                    lines.push(format!("(roh {} × {})", fmt(y), fmt(s.factor)));
                }
                let painter = ui.ctx().layer_painter(egui::LayerId::new(
                    egui::Order::Tooltip,
                    egui::Id::new("hit_tip"),
                ));
                let font = egui::FontId::proportional(14.0);
                let galleys: Vec<_> = lines
                    .iter()
                    .enumerate()
                    .map(|(k, t)| {
                        let col = if k == 0 { s.color } else { Color32::from_gray(30) };
                        painter.layout_no_wrap(t.clone(), font.clone(), col)
                    })
                    .collect();
                let w = galleys.iter().map(|g| g.size().x).fold(0.0, f32::max) + 12.0;
                let h: f32 = galleys.iter().map(|g| g.size().y).sum::<f32>() + 10.0;
                let scr = ui.ctx().content_rect();
                let mut o = pos + egui::vec2(14.0, 14.0);
                if o.x + w > scr.right() {
                    o.x = pos.x - w - 14.0;
                }
                if o.y + h > scr.bottom() {
                    o.y = pos.y - h - 14.0;
                }
                let rect = egui::Rect::from_min_size(o, egui::vec2(w, h));
                painter.rect_filled(rect, 4.0, Color32::from_rgba_unmultiplied(255, 255, 255, 240));
                painter.rect_stroke(rect, 4.0, egui::Stroke::new(1.0, s.color), egui::StrokeKind::Inside);
                let mut y0 = o.y + 5.0;
                for g in galleys {
                    let hh = g.size().y;
                    painter.galley(egui::pos2(o.x + 6.0, y0), g, Color32::BLACK);
                    y0 += hh;
                }
            }
        });

        if !ctx.input(|i| i.raw.hovered_files.is_empty()) {
            let painter = ctx.layer_painter(egui::LayerId::new(
                egui::Order::Foreground,
                egui::Id::new("drop"),
            ));
            let r = ctx.content_rect();
            painter.rect_filled(r, 0.0, Color32::from_black_alpha(160));
            painter.text(
                r.center(),
                egui::Align2::CENTER_CENTER,
                "Loslassen zum Öffnen",
                egui::FontId::proportional(28.0),
                Color32::WHITE,
            );
        }
    }
}

#[cfg(windows)]
#[link(name = "user32")]
unsafe extern "system" {
    fn MessageBoxW(h: isize, text: *const u16, caption: *const u16, ty: u32) -> i32;
}

fn error_box(text: &str) {
    #[cfg(windows)]
    {
        let w = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<u16>>();
        unsafe {
            MessageBoxW(0, w(text).as_ptr(), w("Fig Viewer").as_ptr(), 0x10);
        }
    }
    #[cfg(not(windows))]
    eprintln!("{text}");
}

fn start(renderer: eframe::Renderer, arg: Option<String>) -> eframe::Result {
    let opts = eframe::NativeOptions {
        renderer,
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1400.0, 800.0])
            .with_title("Fig Viewer")
            .with_drag_and_drop(true),
        ..Default::default()
    };
    eframe::run_native(
        "Fig Viewer",
        opts,
        Box::new(move |_cc| {
            let mut app = App::default();
            if let Some(p) = arg {
                app.open(&p);
            }
            Ok(Box::new(app))
        }),
    )
}

fn main() {
    let arg = std::env::args().nth(1);
    // Erst DirectX/Vulkan (wgpu), bei Problemen OpenGL.
    let mut errors = Vec::new();
    for r in [eframe::Renderer::Wgpu, eframe::Renderer::Glow] {
        let a = arg.clone();
        match std::panic::catch_unwind(move || start(r, a)) {
            Ok(Ok(())) => return,
            Ok(Err(e)) => errors.push(format!("{r:?}: {e}")),
            Err(_) => errors.push(format!("{r:?}: Absturz bei der Initialisierung")),
        }
    }
    error_box(&format!(
        "Fig Viewer konnte kein Fenster öffnen.

{}",
        errors.join("
")
    ));
}
