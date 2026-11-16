//! 표·코드 블록·가로줄을 편집창 뒤에 진짜 박스/격자로 그린다.
//! 편집창의 글자는 원문 그대로이고(투명 처리), 여기서는 그 줄들의 위치에 맞춰 그림만 만든다.
use crate::md::{self, Align, Block, BlockKind};
use crate::theme::Theme;
use eframe::egui::{self, Color32, CornerRadius, FontFamily, FontId, Galley, Pos2, Rect, Shape, Stroke, StrokeKind, pos2};
use std::sync::Arc;

/// 칸 안쪽 좌우 여백.
const PAD: f32 = 8.0;

/// 블록 구간 캐시의 키: 본문이 같으면 같다.
pub fn cache_key(text: &str) -> u64 {
    // 8바이트씩 곱하고 섞는 빠른 해시(SipHash보다 훨씬 빠름). 매 프레임 호출되므로 긴 문서에서 중요하다.
    const K: u64 = 0x9E37_79B9_7F4A_7C15;
    let b = text.as_bytes();
    let mut h: u64 = b.len() as u64 ^ 0xcbf2_9ce4_8422_2325;
    let mut chunks = b.chunks_exact(8);
    for c in &mut chunks {
        let v = u64::from_le_bytes([c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7]]);
        h = (h ^ v).wrapping_mul(K).rotate_left(29);
    }
    for &x in chunks.remainder() {
        h = (h ^ x as u64).wrapping_mul(K).rotate_left(11);
    }
    h ^ (h >> 32)
}

/// 본문의 블록 구간. 본문이 바뀔 때만 다시 계산한다(egui 메모리에 캐시).
#[allow(deprecated)]
pub fn blocks_cached(ctx: &egui::Context, text: &str) -> Arc<Vec<Block>> {
    let id = egui::Id::new("akashic_block_cache");
    let key = cache_key(text);
    if let Some((k, b)) = ctx.data_mut(|d| d.get_temp::<(u64, Arc<Vec<Block>>)>(id)) {
        if k == key {
            return b;
        }
    }
    let b = Arc::new(md::scan_blocks(text));
    ctx.data_mut(|d| d.insert_temp(id, (key, b.clone())));
    b
}

/// 줄(논리 줄)마다 galley 안의 (위, 아래) y. 줄바꿈으로 여러 행이 되면 합친다.
pub fn line_spans(galley: &Galley) -> Vec<(f32, f32)> {
    let mut out: Vec<(f32, f32)> = Vec::new();
    let mut cur: Option<(f32, f32)> = None;
    for r in &galley.rows {
        let (a, b) = (r.pos.y, r.pos.y + r.size.y);
        cur = Some(match cur {
            Some((x, y)) => (x.min(a), y.max(b)),
            None => (a, b),
        });
        if r.ends_with_newline {
            out.push(cur.take().unwrap());
        }
    }
    if let Some(c) = cur {
        out.push(c);
    }
    out
}

/// 논리 줄 `start..=end`에서 글자가 차지하는 가장 오른쪽 x(galley 좌표).
pub fn block_text_width(galley: &Galley, start: usize, end: usize) -> f32 {
    let mut n = 0;
    let mut w = 0.0f32;
    for r in &galley.rows {
        if n >= start && n <= end {
            w = w.max(r.pos.x + r.size.x);
        }
        if r.ends_with_newline {
            n += 1;
        }
    }
    w
}
/// 논리 줄 `line`의 `col`번째 글자의 galley 안 x 좌표(줄바꿈된 줄은 첫 행 기준).
pub fn line_glyph_x(galley: &Galley, line: usize, col: usize) -> Option<f32> {
    let mut n = 0;
    for r in &galley.rows {
        if n == line {
            return r.glyphs.get(col).map(|g| r.pos.x + g.pos.x);
        }
        if r.ends_with_newline {
            n += 1;
        }
    }
    None
}
/// 칸 글자를 `maxw` 안에 들어가게 잘라(…) 배치한다.
fn fit(ui: &egui::Ui, text: &str, font: &FontId, color: Color32, maxw: f32) -> Arc<Galley> {
    let lay = |s: String| ui.ctx().fonts_mut(|f| f.layout_no_wrap(s, font.clone(), color));
    let g = lay(text.to_string());
    if g.size().x <= maxw {
        return g;
    }
    let chars: Vec<char> = text.chars().collect();
    let (mut lo, mut hi) = (0usize, chars.len());
    while lo < hi {
        let mid = (lo + hi + 1) / 2;
        let s: String = chars[..mid].iter().collect::<String>() + "…";
        if lay(s).size().x <= maxw {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    lay(chars[..lo].iter().collect::<String>() + "…")
}

/// 편집창 뒤에 깔 그림을 만든다. `hide`가 꺼져 있으면 아무것도 그리지 않는다.
/// `galley_origin`: galley의 (0,0)이 화면 어디인지. `text_rect`: 글이 놓이는 영역(좌우 끝).
pub fn paint(ui: &egui::Ui, blocks: &[Block], galley: &Galley, galley_origin: Pos2, text_rect: Rect, cl: Option<usize>, t: &Theme, size: f32, hide: bool) -> Vec<Shape> {
    let mut shapes = Vec::new();
    if !hide || blocks.is_empty() {
        return shapes;
    }
    let spans = line_spans(galley);
    let view = ui.clip_rect();
    let px = |v: f32| ui.painter().round_to_pixel_center(v);
    let (left, right) = (text_rect.left() - 3.0, text_rect.right() + 3.0);
    let r0 = CornerRadius::ZERO;
    for b in blocks {
        let (Some(first), Some(last)) = (spans.get(b.start), spans.get(b.end.min(spans.len().saturating_sub(1)))) else { continue };
        let top = px(galley_origin.y + first.0);
        let bottom = px(galley_origin.y + last.1);
        if bottom < view.top() - 2.0 || top > view.bottom() + 2.0 {
            continue; // 보이지 않는 블록은 그리지 않는다
        }
        let inside = cl.map_or(false, |c| c >= b.start && c <= b.end);
        match &b.kind {
            BlockKind::Code { lang, .. } => {
                // 짧은 코드는 가장 긴 줄 기준으로 상자 폭을 줄인다(줄바꿈으로 꽉 찬 경우만 끝까지).
                let need = block_text_width(galley, b.start, b.end);
                let label_w = if lang.is_empty() { 0.0 } else { ui.ctx().fonts_mut(|f| f.layout_no_wrap(lang.clone(), FontId::new((size * 0.75).max(10.0), FontFamily::Proportional), t.dim)).size().x + 14.0 };
                let box_right = (galley_origin.x + need + 16.0).max(left + 90.0).max(left + label_w + 40.0).min(right);
                let rect = Rect::from_min_max(pos2(px(left), top), pos2(px(box_right), bottom));
                if inside {
                    shapes.push(Shape::rect_stroke(rect, r0, Stroke::new(1.0, t.border), StrokeKind::Inside));
                } else {
                    shapes.push(Shape::rect_filled(rect, r0, t.code_bg));
                    shapes.push(Shape::rect_stroke(rect, r0, Stroke::new(1.0, t.border), StrokeKind::Inside));
                    if !lang.is_empty() {
                        let g = ui.ctx().fonts_mut(|f| f.layout_no_wrap(lang.clone(), FontId::new((size * 0.75).max(10.0), FontFamily::Proportional), t.dim));
                        let rowh = first.1 - first.0;
                        let p = pos2(rect.right() - 6.0 - g.size().x, rect.top() + ((rowh - g.size().y) / 2.0).max(1.0));
                        shapes.push(Shape::galley(p, g, t.dim));
                    }
                }
            }
            BlockKind::Hr => {
                if inside {
                    continue;
                }
                let y = px((top + bottom) / 2.0);
                shapes.push(Shape::line_segment([pos2(left, y), pos2(right, y)], Stroke::new(1.0, t.dim)));
            }
            BlockKind::Check { done, col } => {
                // 진짜 체크박스(직각): 글자 "[ ]"는 투명하게 두고 그 자리에 칸을 그린다.
                let Some(x0) = line_glyph_x(galley, b.start, *col) else { continue };
                let rowh = last.1 - first.0;
                let side = (size * 0.82).min(rowh - 2.0).max(8.0);
                let cx = galley_origin.x + x0 + side / 2.0 + 1.0;
                let cy = px(galley_origin.y + (first.0 + last.1) / 2.0 + 1.0);
                let rect = Rect::from_center_size(pos2(px(cx), cy), egui::vec2(side, side));
                if *done {
                    shapes.push(Shape::rect_filled(rect, r0, t.accent));
                    let (w, h) = (side, side);
                    let o = rect.min;
                    let pts = vec![pos2(o.x + w * 0.22, o.y + h * 0.52), pos2(o.x + w * 0.42, o.y + h * 0.72), pos2(o.x + w * 0.80, o.y + h * 0.28)];
                    shapes.push(Shape::line(pts, Stroke::new((side * 0.14).max(1.5), t.bg)));
                } else {
                    shapes.push(Shape::rect_filled(rect, r0, t.bg));
                    shapes.push(Shape::rect_stroke(rect, r0, Stroke::new(1.5, t.dim), StrokeKind::Inside));
                }
            }            BlockKind::Table { aligns, rows } => {
                let rect = Rect::from_min_max(pos2(px(left), top), pos2(px(right), bottom));
                if inside {
                    shapes.push(Shape::rect_stroke(rect, r0, Stroke::new(1.0, t.border), StrokeKind::Inside));
                    continue;
                }
                table(ui, &mut shapes, b, aligns, rows, &spans, galley_origin, left, right, t, size, &px);
            }
        }
    }
    shapes
}

#[allow(clippy::too_many_arguments)]
fn table(ui: &egui::Ui, shapes: &mut Vec<Shape>, b: &Block, aligns: &[Align], rows: &[Vec<String>], spans: &[(f32, f32)], origin: Pos2, left: f32, right: f32, t: &Theme, size: f32, px: &dyn Fn(f32) -> f32) {
    let ncols = aligns.len();
    if ncols == 0 || rows.is_empty() {
        return;
    }
    let body = FontId::new(size, FontFamily::Proportional);
    let bold = FontId::new(size, FontFamily::Name("bold".into()));
    let font = |r: usize| if r == 0 { bold.clone() } else { body.clone() };
    // 열 폭 = 셀 내용 최대 폭 + 좌우 여백. 너무 넓으면 같은 비율로 줄이고 글자를 …으로 자른다.
    let mut widths = vec![PAD * 2.0; ncols];
    for (r, row) in rows.iter().enumerate() {
        for (c, cell) in row.iter().enumerate().take(ncols) {
            let w = ui.ctx().fonts_mut(|f| f.layout_no_wrap(cell.clone(), font(r), t.text)).size().x;
            widths[c] = widths[c].max(w + PAD * 2.0);
        }
    }
    let avail = (right - left - 2.0).max(40.0);
    let total: f32 = widths.iter().sum();
    if total > avail {
        let k = avail / total;
        // 열이 아주 많으면 최소 폭 합계가 본문 폭을 넘으므로, 최소 폭도 한 칸 몫(avail/열 수)을 넘지 않게 한다.
        let min_w = (PAD * 2.0 + 12.0).min(avail / ncols as f32);
        for w in widths.iter_mut() {
            *w = (*w * k).max(min_w);
        }
        // 최소 폭 보정으로 합이 다시 넘으면 한 번 더 같은 비율로 줄인다.
        let sum: f32 = widths.iter().sum();
        if sum > avail {
            let k2 = avail / sum;
            for w in widths.iter_mut() {
                *w *= k2;
            }
        }
    }
    let total: f32 = widths.iter().sum();
    let x0 = px(left);
    let mut xs = vec![x0];
    for w in &widths {
        xs.push(px(xs.last().unwrap() + w));
    }
    let x1 = *xs.last().unwrap();
    let line_y = |l: usize, bottom: bool| px(origin.y + spans.get(l).map_or(0.0, |s| if bottom { s.1 } else { s.0 }));
    let (top, bottom) = (line_y(b.start, false), line_y(b.end, true));
    let _ = total;
    // 헤더 배경
    let hb = line_y(b.start + 1, true);
    shapes.push(Shape::rect_filled(Rect::from_min_max(pos2(x0, top), pos2(x1, hb)), CornerRadius::ZERO, t.hover));
    // 안쪽 가로줄(행 사이)과 세로줄
    let thin = Stroke::new(1.0, t.border);
    for r in 2..rows.len() {
        let y = line_y(b.start + 1 + r, false);
        shapes.push(Shape::line_segment([pos2(x0, y), pos2(x1, y)], thin));
    }
    for x in &xs[1..xs.len() - 1] {
        shapes.push(Shape::line_segment([pos2(*x, top), pos2(*x, bottom)], thin));
    }
    // 헤더 아래 굵은 선(구분선 줄 자리)
    let (s0, s1) = (line_y(b.start + 1, false), line_y(b.start + 1, true));
    let ym = (s0 + s1) / 2.0;
    shapes.push(Shape::line_segment([pos2(x0, ym), pos2(x1, ym)], Stroke::new(2.0, t.dim)));
    // 바깥 테두리
    shapes.push(Shape::rect_stroke(Rect::from_min_max(pos2(x0, top), pos2(x1, bottom)), CornerRadius::ZERO, Stroke::new(1.0, t.dim), StrokeKind::Inside));
    // 칸 글자
    for (r, row) in rows.iter().enumerate() {
        let line = if r == 0 { b.start } else { b.start + 1 + r };
        let (ya, yb) = (line_y(line, false), line_y(line, true));
        for (c, cell) in row.iter().enumerate().take(ncols) {
            if cell.is_empty() {
                continue;
            }
            let color = t.text;
            let g = fit(ui, cell, &font(r), color, widths[c] - PAD * 2.0);
            let w = g.size().x;
            let inner = widths[c] - PAD * 2.0;
            let dx = match aligns[c] {
                Align::Left => 0.0,
                Align::Center => (inner - w) / 2.0,
                Align::Right => inner - w,
            };
            let p = pos2(xs[c] + PAD + dx.max(0.0), (ya + yb) / 2.0 - g.size().y / 2.0);
            shapes.push(Shape::galley(p, g, color));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_key_follows_text() {
        assert_eq!(cache_key("a\nb"), cache_key("a\nb"));
        assert_ne!(cache_key("a\nb"), cache_key("a\nb "));
    }
}

#[cfg(test)]
mod perf_tests {
    use super::*;
    use eframe::egui::{Context, RawInput};
    use std::time::Instant;

    /// 아주 긴 문서에서 프레임당 비용을 부분별로 잰다(`cargo test --release perf_long -- --ignored --nocapture`).
    #[test]
    #[ignore]
    fn perf_long_doc_frame_cost() {
        let mut doc = String::new();
        for i in 0..30000 {
            doc.push_str(&format!("{i} 줄입니다. 한글과 English가 섞인 본문 **굵게** 와 `코드` 그리고 [[링크]] 가 있습니다.\n"));
        }
        println!("문서 {}줄, {:.1}MB", 30000, doc.len() as f64 / 1e6);
        let t = Instant::now();
        for _ in 0..20 {
            std::hint::black_box(cache_key(&doc));
        }
        println!("cache_key(SipHash) 1회: {:.2} ms", t.elapsed().as_secs_f64() * 1000.0 / 20.0);
        let t = Instant::now();
        let theme = crate::theme::themes()[0].clone();
        let job = md::build(&doc, Some(5), 15.0, &theme, 700.0, true);
        println!("md::build 1회: {:.0} ms", t.elapsed().as_secs_f64() * 1000.0);
        let ctx = Context::default();
        {
            let mut raw = RawInput::default();
            raw.screen_rect = Some(eframe::egui::Rect::from_min_size(eframe::egui::Pos2::ZERO, eframe::egui::vec2(800.0, 600.0)));
            let _ = ctx.run_ui(raw, |_ui| {});
        }
        let t = Instant::now();
        let galley = ctx.fonts_mut(|f| f.layout_job(job));
        println!("글자 배치(layout_job) 1회: {:.0} ms (줄 {}행)", t.elapsed().as_secs_f64() * 1000.0, galley.rows.len());
        let t = Instant::now();
        for _ in 0..20 {
            std::hint::black_box(line_spans(&galley));
        }
        println!("line_spans 1회: {:.2} ms", t.elapsed().as_secs_f64() * 1000.0 / 20.0);
        let t = Instant::now();
        for _ in 0..20 {
            std::hint::black_box(md::line_of_char(&doc, doc.len() / 2));
        }
        println!("line_of_char 1회: {:.2} ms", t.elapsed().as_secs_f64() * 1000.0 / 20.0);
        let t = Instant::now();
        std::hint::black_box(md::scan_blocks(&doc));
        println!("scan_blocks 1회: {:.0} ms", t.elapsed().as_secs_f64() * 1000.0);

        // 편집창(스크롤 영역 + TextEdit)을 캐시된 galley로 그리는 프레임
        let mut body = doc.clone();
        let mut run = |scroll: f32| {
            let mut raw = RawInput::default();
            raw.screen_rect = Some(eframe::egui::Rect::from_min_size(eframe::egui::Pos2::ZERO, eframe::egui::vec2(800.0, 600.0)));
            let g = galley.clone();
            let out = ctx.run_ui(raw, |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    egui::ScrollArea::vertical().vertical_scroll_offset(scroll).auto_shrink([false, false]).show(ui, |ui| {
                        let mut lay = |_ui: &egui::Ui, _b: &dyn egui::TextBuffer, _w: f32| g.clone();
                        egui::TextEdit::multiline(&mut body).desired_width(f32::INFINITY).layouter(&mut lay).show(ui);
                    });
                });
            });
            let t = Instant::now();
            let prims = ctx.tessellate(out.shapes, out.pixels_per_point);
            (t.elapsed().as_secs_f64() * 1000.0, prims.len())
        };
        run(0.0);
        let t = Instant::now();
        let mut tess = 0.0;
        for k in 0..20 {
            let (tt, _) = run(10000.0 + k as f32 * 37.0);
            tess += tt;
        }
        println!("편집창 프레임(스크롤 중) 1회 전체: {:.2} ms (그중 tessellate {:.2} ms)", t.elapsed().as_secs_f64() * 1000.0 / 20.0, tess / 20.0);
    }
}
