use eframe::egui::{self, Color32, Pos2, Stroke, Vec2};
pub const INK: Color32 = Color32::from_rgb(48, 58, 76);
pub const MUTED: Color32 = Color32::from_rgb(122, 133, 151);
pub const BLUE: Color32 = Color32::from_rgb(24, 142, 216);
pub const LINE: Color32 = Color32::from_rgb(225, 230, 237);
#[derive(Clone, Copy)]
pub enum Icon {
    Open,
    Print,
    Minus,
    Plus,
    Rotate,
    First,
    Prev,
    Next,
    Last,
    Width,
    Fit,
    Actual,
    Continuous,
    Single,
    Search,
    Outline,
    Image,
    Attach,
    Check,
    Info,
    Fullscreen,
    Save,
}
pub fn icon(painter: &egui::Painter, rect: egui::Rect, kind: Icon, color: Color32) {
    let center = rect.center();
    let point = |x: f32, y: f32| center + Vec2::new(x, y);
    let stroke = Stroke::new(1.35, color);
    let line = |a: (f32, f32), b: (f32, f32)| {
        painter.line_segment([point(a.0, a.1), point(b.0, b.1)], stroke);
    };
    let path = |ps: &[(f32, f32)]| {
        painter.add(egui::Shape::line(
            ps.iter().map(|p| point(p.0, p.1)).collect(),
            stroke,
        ));
    };
    let box_at = |x: f32, y: f32, w: f32, h: f32| {
        painter.rect_stroke(
            egui::Rect::from_min_size(point(x, y), Vec2::new(w, h)),
            1.,
            stroke,
            egui::StrokeKind::Inside,
        );
    };
    match kind {
        Icon::Open => {
            path(&[
                (-9., 7.),
                (-9., -6.),
                (-3., -6.),
                (-1., -3.),
                (9., -3.),
                (7., 7.),
                (-9., 7.),
            ]);
        }
        Icon::Print => {
            box_at(-9., -4., 18., 11.);
            box_at(-6., -10., 12., 7.);
            painter.rect_filled(
                egui::Rect::from_min_max(point(-6., 2.), point(6., 10.)),
                0.,
                Color32::WHITE,
            );
            box_at(-6., 2., 12., 8.);
            line((-3., 5.), (3., 5.));
        }
        Icon::Plus | Icon::Minus => {
            painter.circle_stroke(center, 9., stroke);
            line((-4., 0.), (4., 0.));
            if matches!(kind, Icon::Plus) {
                line((0., -4.), (0., 4.));
            }
        }
        Icon::Rotate => {
            painter.add(egui::Shape::line(
                (0..20)
                    .map(|i| {
                        let a = -2.8 + i as f32 * 0.25;
                        point(a.cos() * 9., a.sin() * 9.)
                    })
                    .collect(),
                stroke,
            ));
            path(&[(-10., -6.), (-8., 1.), (-2., -3.)]);
        }
        Icon::Prev | Icon::First => {
            path(&[(3., -7.), (-3., 0.), (3., 7.)]);
            if matches!(kind, Icon::First) {
                line((-8., -8.), (-8., 8.));
            }
        }
        Icon::Next | Icon::Last => {
            path(&[(-3., -7.), (3., 0.), (-3., 7.)]);
            if matches!(kind, Icon::Last) {
                line((8., -8.), (8., 8.));
            }
        }
        Icon::Width | Icon::Fit | Icon::Actual => {
            box_at(-7., -10., 14., 20.);
            if matches!(kind, Icon::Actual) {
                painter.text(
                    center,
                    egui::Align2::CENTER_CENTER,
                    "1:1",
                    egui::FontId::proportional(8.),
                    color,
                );
            } else {
                line((-11., 0.), (11., 0.));
                path(&[(-8., -3.), (-11., 0.), (-8., 3.)]);
                path(&[(8., -3.), (11., 0.), (8., 3.)]);
                if matches!(kind, Icon::Fit) {
                    line((0., -13.), (0., 13.));
                }
            }
        }
        Icon::Continuous => {
            box_at(-7., -11., 14., 9.);
            box_at(-7., 2., 14., 9.);
            line((-3., -7.), (3., -7.));
            line((-3., 6.), (3., 6.));
        }
        Icon::Single => {
            box_at(-7., -10., 14., 20.);
            for y in [-4., 0., 4.] {
                line((-3., y), (3., y));
            }
        }
        Icon::Search => {
            painter.circle_stroke(point(-2., -2.), 6.5, stroke);
            line((3., 3.), (9., 9.));
        }
        Icon::Outline => {
            for y in [-6., 0., 6.] {
                line((-3., y), (9., y));
                painter.circle_filled(point(-8., y), 1., color);
            }
        }
        Icon::Image => {
            box_at(-10., -8., 20., 16.);
            path(&[(-8., 6.), (-2., -1.), (2., 3.), (6., -2.), (9., 4.)]);
            painter.circle_stroke(point(-5., -4.), 1.5, stroke);
        }
        Icon::Attach => {
            path(&[
                (7., 0.),
                (0., 7.),
                (-5., 7.),
                (-8., 4.),
                (-8., 0.),
                (1., -9.),
                (5., -9.),
                (8., -6.),
                (8., -2.),
                (-1., 7.),
            ]);
            path(&[(4., -5.), (-4., 3.), (-3., 5.), (-1., 5.), (5., -1.)]);
        }
        Icon::Check => {
            path(&[(-8., 0.), (-2., 6.), (9., -7.)]);
        }
        Icon::Info => {
            painter.circle_stroke(center, 9., stroke);
            line((0., -1.), (0., 5.));
            painter.circle_filled(point(0., -5.), 1., color);
        }
        Icon::Fullscreen => {
            for (sx, sy) in [(-1., -1.), (1., -1.), (-1., 1.), (1., 1.)] {
                path(&[(sx * 4., sy * 9.), (sx * 9., sy * 9.), (sx * 9., sy * 4.)]);
            }
        }
        Icon::Save => {
            box_at(-8., -9., 16., 18.);
            box_at(-4., -9., 8., 7.);
            box_at(-4., 3., 8., 6.);
        }
    }
}
pub fn tool(
    ui: &mut egui::Ui,
    kind: Icon,
    label: &str,
    selected: bool,
    enabled: bool,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(57., 62.), egui::Sense::click());
    if selected || response.hovered() {
        ui.painter().rect_filled(
            rect.shrink(2.),
            5.,
            if selected {
                Color32::from_rgb(230, 243, 253)
            } else {
                Color32::from_rgb(242, 246, 250)
            },
        );
    }
    let color = if !enabled {
        Color32::from_rgb(190, 198, 208)
    } else if selected {
        BLUE
    } else {
        INK
    };
    icon(
        ui.painter(),
        egui::Rect::from_center_size(
            Pos2::new(rect.center().x, rect.top() + 22.),
            Vec2::splat(23.),
        ),
        kind,
        color,
    );
    ui.painter().text(
        Pos2::new(rect.center().x, rect.top() + 48.),
        egui::Align2::CENTER_CENTER,
        label,
        egui::FontId::proportional(12.),
        color,
    );
    if !enabled {
        response.on_disabled_hover_text("请先打开文档")
    } else {
        response
    }
}
pub fn rail(ui: &mut egui::Ui, kind: Icon, label: &str, selected: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(36., 38.), egui::Sense::click());
    if selected {
        ui.painter()
            .rect_filled(rect.shrink(3.), 5., Color32::from_rgb(225, 240, 252));
    }
    if response.hovered() {
        ui.painter()
            .rect_filled(rect.shrink(3.), 5., Color32::from_rgb(231, 236, 243));
    }
    icon(
        ui.painter(),
        egui::Rect::from_center_size(rect.center(), Vec2::splat(20.)),
        kind,
        if selected { BLUE } else { MUTED },
    );
    response.on_hover_text(label)
}
pub fn divider(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(9., 48.), egui::Sense::hover());
    ui.painter().line_segment(
        [rect.center_top(), rect.center_bottom()],
        Stroke::new(1., LINE),
    );
}
