//! 테마 색을 따라가는 선 아이콘(직각 위주). 이모지 대신 직접 그린다.
//! 모든 아이콘은 0..1 정사각형 좌표로 정의하고, 글자 색(버튼 상태에 따른 색)으로 선만 긋는다.
use eframe::egui::{self, pos2, Color32, Rect, Response, Ui, Vec2};

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Icon {
    NewDoc,
    Sidebar,
    Back,
    Forward,
    Options,
    Help,
    Trash,
    Restore,
    Cross,
    Star,
    Folder,
    Save,
    Sync,
    Export,
    Import,
    Check,
    Edit,
    Plus,
    Merge,
    Search,
    Database,
    Detach,
    Attach,
    Up,
    Down,
    Copy,
    Home,
    Keyboard,
    Pointer,
    Graph,
}

/// 아이콘 한 변(글자 크기에 비례).
pub fn side(ui: &Ui) -> f32 {
    ui.text_style_height(&egui::TextStyle::Button).max(12.0) * 0.95
}

/// 아이콘을 이루는 도형(0..1 좌표).
enum Prim {
    /// 꺾은선(참이면 닫힌 다각형)
    Line(Vec<[f32; 2]>, bool),
    /// 속이 찬 정사각형: 중심 x, y, 한 변
    Rect(f32, f32, f32),
    /// 원 테두리: 중심 x, y, 반지름
    Circle(f32, f32, f32),
}

fn prims(icon: Icon) -> Vec<Prim> {
    let out = std::cell::RefCell::new(Vec::<Prim>::new());
    let q = |x: f32, y: f32| [x, y];
    let line = |pts: &[[f32; 2]]| out.borrow_mut().push(Prim::Line(pts.to_vec(), false));
    let poly = |pts: &[[f32; 2]]| out.borrow_mut().push(Prim::Line(pts.to_vec(), true));
    let rect = |x: f32, y: f32, k: f32| out.borrow_mut().push(Prim::Rect(x, y, k));
    let circle = |x: f32, y: f32, rad: f32| out.borrow_mut().push(Prim::Circle(x, y, rad));    match icon {
        Icon::NewDoc => {
            poly(&[q(0.2, 0.06), q(0.62, 0.06), q(0.84, 0.28), q(0.84, 0.94), q(0.2, 0.94)]);
            line(&[q(0.62, 0.06), q(0.62, 0.28), q(0.84, 0.28)]);
            line(&[q(0.52, 0.5), q(0.52, 0.8)]);
            line(&[q(0.37, 0.65), q(0.67, 0.65)]);
        }
        Icon::Sidebar => {
            poly(&[q(0.08, 0.15), q(0.92, 0.15), q(0.92, 0.85), q(0.08, 0.85)]);
            line(&[q(0.38, 0.15), q(0.38, 0.85)]);
        }
        Icon::Back => {
            line(&[q(0.9, 0.5), q(0.1, 0.5)]);
            line(&[q(0.4, 0.2), q(0.1, 0.5), q(0.4, 0.8)]);
        }
        Icon::Forward => {
            line(&[q(0.1, 0.5), q(0.9, 0.5)]);
            line(&[q(0.6, 0.2), q(0.9, 0.5), q(0.6, 0.8)]);
        }
        Icon::Options => {
            // 슬라이더 세 줄
            for (y, x) in [(0.22, 0.65), (0.5, 0.32), (0.78, 0.55)] {
                line(&[q(0.08, y), q(0.92, y)]);
                rect(x, y, 0.22);
            }
        }
        Icon::Help => {
            circle(0.5, 0.5, 0.44);
            line(&[q(0.36, 0.38), q(0.4, 0.28), q(0.6, 0.28), q(0.64, 0.4), q(0.5, 0.52), q(0.5, 0.62)]);
            rect(0.5, 0.76, 0.1);
        }
        Icon::Trash => {
            line(&[q(0.1, 0.22), q(0.9, 0.22)]);
            line(&[q(0.36, 0.22), q(0.36, 0.08), q(0.64, 0.08), q(0.64, 0.22)]);
            poly(&[q(0.2, 0.22), q(0.8, 0.22), q(0.74, 0.94), q(0.26, 0.94)]);
            line(&[q(0.42, 0.4), q(0.42, 0.76)]);
            line(&[q(0.58, 0.4), q(0.58, 0.76)]);
        }
        Icon::Restore => {
            line(&[q(0.2, 0.2), q(0.2, 0.46), q(0.46, 0.46)]);
            line(&[q(0.2, 0.46), q(0.3, 0.3), q(0.5, 0.2), q(0.72, 0.26), q(0.86, 0.46), q(0.84, 0.7), q(0.66, 0.86), q(0.44, 0.88)]);
        }
        Icon::Cross => {
            line(&[q(0.2, 0.2), q(0.8, 0.8)]);
            line(&[q(0.8, 0.2), q(0.2, 0.8)]);
        }
        Icon::Star => {
            let pts: Vec<[f32; 2]> = (0..10)
                .map(|i| {
                    let a = -std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::PI / 5.0;
                    let rad = if i % 2 == 0 { 0.46 } else { 0.2 };
                    q(0.5 + rad * a.cos(), 0.54 + rad * a.sin())
                })
                .collect();
            poly(&pts);
        }
        Icon::Folder => {
            poly(&[q(0.06, 0.2), q(0.38, 0.2), q(0.48, 0.32), q(0.94, 0.32), q(0.94, 0.84), q(0.06, 0.84)]);
        }
        Icon::Save => {
            poly(&[q(0.1, 0.1), q(0.78, 0.1), q(0.9, 0.22), q(0.9, 0.9), q(0.1, 0.9)]);
            poly(&[q(0.3, 0.1), q(0.3, 0.38), q(0.68, 0.38), q(0.68, 0.1)]);
            poly(&[q(0.28, 0.9), q(0.28, 0.6), q(0.72, 0.6), q(0.72, 0.9)]);
        }
        Icon::Sync => {
            line(&[q(0.14, 0.4), q(0.3, 0.2), q(0.7, 0.2), q(0.86, 0.4)]);
            line(&[q(0.86, 0.15), q(0.86, 0.4), q(0.62, 0.4)]);
            line(&[q(0.86, 0.6), q(0.7, 0.8), q(0.3, 0.8), q(0.14, 0.6)]);
            line(&[q(0.14, 0.85), q(0.14, 0.6), q(0.38, 0.6)]);
        }
        Icon::Export => {
            line(&[q(0.5, 0.62), q(0.5, 0.1)]);
            line(&[q(0.3, 0.3), q(0.5, 0.1), q(0.7, 0.3)]);
            line(&[q(0.12, 0.55), q(0.12, 0.9), q(0.88, 0.9), q(0.88, 0.55)]);
        }
        Icon::Import => {
            line(&[q(0.5, 0.1), q(0.5, 0.62)]);
            line(&[q(0.3, 0.42), q(0.5, 0.62), q(0.7, 0.42)]);
            line(&[q(0.12, 0.55), q(0.12, 0.9), q(0.88, 0.9), q(0.88, 0.55)]);
        }
        Icon::Check => {
            line(&[q(0.12, 0.52), q(0.4, 0.8), q(0.9, 0.22)]);
        }
        Icon::Edit => {
            poly(&[q(0.62, 0.12), q(0.88, 0.38), q(0.34, 0.92), q(0.1, 0.92), q(0.1, 0.68)]);
            line(&[q(0.52, 0.22), q(0.78, 0.48)]);
        }
        Icon::Plus => {
            line(&[q(0.5, 0.15), q(0.5, 0.85)]);
            line(&[q(0.15, 0.5), q(0.85, 0.5)]);
        }
        Icon::Merge => {
            line(&[q(0.15, 0.1), q(0.15, 0.3), q(0.5, 0.5)]);
            line(&[q(0.85, 0.1), q(0.85, 0.3), q(0.5, 0.5)]);
            line(&[q(0.5, 0.5), q(0.5, 0.88)]);
            line(&[q(0.34, 0.72), q(0.5, 0.88), q(0.66, 0.72)]);
        }
        Icon::Search => {
            poly(&[q(0.12, 0.12), q(0.58, 0.12), q(0.58, 0.58), q(0.12, 0.58)]);
            line(&[q(0.58, 0.58), q(0.9, 0.9)]);
        }
        Icon::Database => {
            poly(&[q(0.14, 0.12), q(0.86, 0.12), q(0.86, 0.88), q(0.14, 0.88)]);
            line(&[q(0.14, 0.38), q(0.86, 0.38)]);
            line(&[q(0.14, 0.62), q(0.86, 0.62)]);
        }
        Icon::Detach => {
            poly(&[q(0.1, 0.3), q(0.7, 0.3), q(0.7, 0.9), q(0.1, 0.9)]);
            line(&[q(0.4, 0.6), q(0.9, 0.1)]);
            line(&[q(0.55, 0.1), q(0.9, 0.1), q(0.9, 0.45)]);
        }
        Icon::Attach => {
            poly(&[q(0.1, 0.1), q(0.9, 0.1), q(0.9, 0.9), q(0.1, 0.9)]);
            line(&[q(0.7, 0.3), q(0.3, 0.7)]);
            line(&[q(0.3, 0.4), q(0.3, 0.7), q(0.6, 0.7)]);
        }
        Icon::Up => {
            line(&[q(0.5, 0.9), q(0.5, 0.12)]);
            line(&[q(0.22, 0.4), q(0.5, 0.12), q(0.78, 0.4)]);
        }
        Icon::Down => {
            line(&[q(0.5, 0.1), q(0.5, 0.88)]);
            line(&[q(0.22, 0.6), q(0.5, 0.88), q(0.78, 0.6)]);
        }
        Icon::Copy => {
            poly(&[q(0.3, 0.3), q(0.88, 0.3), q(0.88, 0.9), q(0.3, 0.9)]);
            line(&[q(0.12, 0.7), q(0.12, 0.1), q(0.66, 0.1)]);
        }
        Icon::Home => {
            line(&[q(0.08, 0.5), q(0.5, 0.12), q(0.92, 0.5)]);
            poly(&[q(0.2, 0.42), q(0.2, 0.88), q(0.8, 0.88), q(0.8, 0.42)]);
        }
        Icon::Keyboard => {
            poly(&[q(0.06, 0.25), q(0.94, 0.25), q(0.94, 0.75), q(0.06, 0.75)]);
            line(&[q(0.22, 0.42), q(0.78, 0.42)]);
            line(&[q(0.3, 0.6), q(0.7, 0.6)]);
        }
        Icon::Pointer => {
            poly(&[q(0.22, 0.1), q(0.22, 0.82), q(0.4, 0.66), q(0.56, 0.92), q(0.66, 0.86), q(0.52, 0.6), q(0.76, 0.6)]);
        }
        Icon::Graph => {
            for (x, y) in [(0.2, 0.25), (0.8, 0.2), (0.5, 0.78)] {
                rect(x, y, 0.2);
            }
            line(&[q(0.2, 0.25), q(0.8, 0.2), q(0.5, 0.78), q(0.2, 0.25)]);
        }
    }
    out.into_inner()
}

fn dist_seg(p: [f32; 2], a: [f32; 2], b: [f32; 2]) -> f32 {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let l2 = dx * dx + dy * dy;
    let t = if l2 <= 1e-9 { 0.0 } else { (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / l2).clamp(0.0, 1.0) };
    let (cx, cy) = (a[0] + t * dx, a[1] + t * dy);
    ((p[0] - cx).powi(2) + (p[1] - cy).powi(2)).sqrt()
}

fn covers(prims: &[Prim], p: [f32; 2], hw: f32) -> bool {
    for pr in prims {
        match pr {
            Prim::Line(pts, closed) => {
                for w in pts.windows(2) {
                    if dist_seg(p, w[0], w[1]) <= hw {
                        return true;
                    }
                }
                if *closed && pts.len() > 2 && dist_seg(p, pts[pts.len() - 1], pts[0]) <= hw {
                    return true;
                }
            }
            Prim::Rect(x, y, k) => {
                if (p[0] - x).abs() <= k / 2.0 && (p[1] - y).abs() <= k / 2.0 {
                    return true;
                }
            }
            Prim::Circle(x, y, rad) => {
                if (((p[0] - x).powi(2) + (p[1] - y).powi(2)).sqrt() - rad).abs() <= hw {
                    return true;
                }
            }
        }
    }
    false
}

/// `n`×`n` 화면 픽셀로 아이콘을 그린다(4×4 초과 표본으로 계단 없이 부드럽게). 흰색 + 투명도 마스크.
fn raster(icon: Icon, n: usize) -> egui::ColorImage {
    const SS: usize = 4;
    let pr = prims(icon);
    let nf = n as f32;
    let hw = (nf * 0.1).max(1.5) / 2.0 / nf;
    let mut px = Vec::with_capacity(n * n);
    for y in 0..n {
        for x in 0..n {
            let mut cov = 0u32;
            for sy in 0..SS {
                for sx in 0..SS {
                    let fx = (x as f32 + (sx as f32 + 0.5) / SS as f32) / nf;
                    let fy = (y as f32 + (sy as f32 + 0.5) / SS as f32) / nf;
                    if covers(&pr, [fx, fy], hw) {
                        cov += 1;
                    }
                }
            }
            px.push(Color32::from_white_alpha((cov * 255 / (SS * SS) as u32) as u8));
        }
    }
    egui::ColorImage::new([n, n], px)
}

type Cache = std::sync::Arc<std::sync::Mutex<std::collections::HashMap<(Icon, usize), egui::TextureHandle>>>;

#[allow(deprecated)]
fn texture(ctx: &egui::Context, icon: Icon, n: usize) -> egui::TextureHandle {
    let cache: Cache = ctx.data_mut(|d| d.get_temp_mut_or_insert_with(egui::Id::new("akashic_icon_cache"), Cache::default).clone());
    let mut g = cache.lock().unwrap();
    g.entry((icon, n)).or_insert_with(|| ctx.load_texture(format!("icon_{icon:?}_{n}"), raster(icon, n), egui::TextureOptions::LINEAR)).clone()
}

/// 정사각형 `r` 안에 아이콘을 그린다(화면 픽셀에 맞춰 만든 이미지를 글자 색으로 칠한다).
pub fn paint(p: &egui::Painter, r: Rect, icon: Icon, c: Color32) {
    let ppp = p.ctx().pixels_per_point();
    let n = ((r.width() * ppp).round() as usize).clamp(8, 256);
    let tex = texture(p.ctx(), icon, n);
    let min = pos2((r.min.x * ppp).round() / ppp, (r.min.y * ppp).round() / ppp);
    let rect = Rect::from_min_size(min, Vec2::splat(n as f32 / ppp));
    p.image(tex.id(), rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), c);
}
/// 아이콘 + 글자 버튼. 반환값은 일반 버튼과 같은 `Response`.
pub fn btn(ui: &mut Ui, icon: Icon, text: &str) -> Response {
    btn_ex(ui, icon, text, true, None)
}

pub fn btn_ex(ui: &mut Ui, icon: Icon, text: &str, enabled: bool, min_size: Option<Vec2>) -> Response {
    let side = side(ui);
    // 글자 앞에 아이콘 자리만큼 공백을 둔다.
    let sp = ui.ctx().fonts_mut(|f| f.layout_no_wrap(" ".into(), egui::TextStyle::Button.resolve(ui.style()), Color32::WHITE).size().x).max(1.0);
    let n = ((side + 4.0) / sp).ceil() as usize;
    let label = if text.is_empty() { " ".repeat(n) } else { format!("{}{}", " ".repeat(n), text) };
    let mut b = egui::Button::new(label);
    if let Some(m) = min_size {
        b = b.min_size(m);
    }
    let r = ui.add_enabled(enabled, b);
    paint_in(ui, &r, icon, side);
    r
}

/// 이미 만든 버튼 `r`의 왼쪽(안쪽 여백) 자리에 아이콘을 그린다.
pub fn paint_in(ui: &Ui, r: &Response, icon: Icon, side: f32) {
    let c = ui.style().interact(r).fg_stroke.color;
    let pad = ui.spacing().button_padding.x;
    let rect = Rect::from_min_size(pos2(r.rect.left() + pad, r.rect.center().y - side / 2.0), Vec2::splat(side));
    paint(ui.painter(), rect, icon, c);
}

/// 글자 없이 아이콘만 있는 작은 버튼.
pub fn icon_only(ui: &mut Ui, icon: Icon) -> Response {
    let side = side(ui);
    let (rect, r) = ui.allocate_exact_size(Vec2::splat(side + 8.0), egui::Sense::click());
    if r.hovered() || r.has_focus() {
        ui.painter().rect_filled(rect, 0.0, ui.visuals().widgets.hovered.bg_fill);
    }
    let c = ui.style().interact(&r).fg_stroke.color;
    paint(ui.painter(), Rect::from_center_size(rect.center(), Vec2::splat(side)), icon, c);
    r
}
