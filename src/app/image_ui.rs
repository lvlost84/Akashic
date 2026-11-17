//! 이미지 삽입 UI: 붙여넣기·끌어놓기·메뉴로 들어온 그림을 별도 스레드에서 줄여 DB에 넣고 본문에 한 줄을 끼운다.
use super::*;
use crate::img::{self, Processed};
use std::collections::VecDeque;
use std::path::Path;
use std::sync::mpsc::{channel, Receiver, TryRecvError};
use std::sync::Mutex;

/// 이미지가 들어오는 곳.
pub(super) enum ImageSrc {
    File(PathBuf),
    Rgba { w: u32, h: u32, data: Vec<u8> },
}

struct Job {
    rx: Receiver<Result<Processed, String>>,
    /// 시작할 때 열려 있던 문서와 그때의 커서 줄(끝나면 이 문서가 아니면 취소)
    doc: i64,
    line: Option<usize>,
    /// 커서가 이미지 줄 위일 때 그 줄에 이어 붙여도 되는 작업인지(묶음의 첫 그림만)
    join: bool,
}

// 앱 구조체를 건드리지 않으려고 모듈 전역으로 둔다(export_ui.rs와 같은 방식).
static JOB: Mutex<Option<Job>> = Mutex::new(None);
/// 여러 파일을 한꺼번에 끌어놓았을 때 차례를 기다리는 파일.
static QUEUE: Mutex<VecDeque<PathBuf>> = Mutex::new(VecDeque::new());
/// 같은 묶음의 다음 그림을 넣을 위치(방금 넣은 줄의 번호): (문서 id, 줄).
static NEXT_LINE: Mutex<Option<(i64, usize)>> = Mutex::new(None);
static CTX: Mutex<Option<egui::Context>> = Mutex::new(None);
/// 옵션 창의 "이미지 N개 · X MB" 표시(2초마다 갱신).
static STATS: Mutex<Option<(Instant, usize, u64)>> = Mutex::new(None);

pub(super) fn is_image_path(p: &Path) -> bool {
    let ext = p.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "bmp" | "gif" | "webp")
}

type Rgba = (u32, u32, Vec<u8>);

/// egui가 클립보드 그림을 ColorImage(곱해진 알파)로 주므로 곱하지 않은 RGBA로 되돌린다.
/// (egui-winit는 Ctrl+V에서 클립보드에 글이 있으면 `Paste`, 글이 없고 그림만 있으면 `PasteImage`만 보내고
/// `V` 키 이벤트는 만들지 않는다 → 글이 우선이라 엑셀·웹 복사 같은 경우는 글 붙여넣기가 된다.)
pub(super) fn color_image_to_rgba(img: &egui::ColorImage) -> Rgba {
    let data = img.pixels.iter().flat_map(|c| c.to_srgba_unmultiplied()).collect();
    (img.size[0] as u32, img.size[1] as u32, data)
}

/// 이번 프레임 입력에서 붙여넣은 그림.
fn find_paste_image(events: &[egui::Event]) -> Option<Arc<egui::ColorImage>> {
    events.iter().find_map(|e| match e {
        egui::Event::PasteImage(im) => Some(im.clone()),
        _ => None,
    })
}

/// `from`줄부터 찾아 `line`과 같은 줄의 번호.
fn line_index_of(body: &str, line: &str, from: usize) -> Option<usize> {
    body.split('\n').enumerate().skip(from).find(|(_, l)| l.trim() == line).map(|(i, _)| i)
}

impl App {
    /// 화면에 쓰는 egui 컨텍스트를 기억해 둔다(작업 스레드가 끝나면 화면을 다시 그리게 하려고).
    fn remember_ctx(ctx: &egui::Context) {
        if let Ok(mut g) = CTX.lock() {
            if g.is_none() {
                *g = Some(ctx.clone());
            }
        }
    }

    fn image_busy() -> bool {
        JOB.lock().map(|g| g.is_some()).unwrap_or(false)
    }

    /// 이미지 하나를 별도 스레드에서 줄이는 작업을 시작한다. 끝나면 `poll_image`가 본문에 넣는다.
    pub(super) fn start_image_job(&mut self, ctx: &egui::Context, src: ImageSrc) {
        Self::remember_ctx(ctx);
        let Some(doc) = self.cur else {
            self.say("이미지를 넣을 문서를 먼저 여세요");
            return;
        };
        if self.cur_trashed {
            self.say("휴지통의 문서에는 이미지를 넣을 수 없습니다");
            return;
        }
        if Self::image_busy() {
            match src {
                ImageSrc::File(p) => {
                    if let Ok(mut q) = QUEUE.lock() {
                        q.push_back(p);
                    }
                }
                ImageSrc::Rgba { .. } => self.say("앞의 이미지를 처리하는 중입니다. 잠시 뒤에 다시 붙여넣으세요"),
            }
            return;
        }
        // 묶음(여러 파일)의 두 번째부터는 앞 그림 줄 아래에 쌓고, 처음 하나만 커서의 이미지 줄에 이어 붙일 수 있다.
        let (line, join) = match NEXT_LINE.lock().ok().and_then(|g| *g) {
            Some((d, l)) if d == doc => (Some(l), false),
            _ => (self.cursor_line, true),
        };
        let (max, quality) = (self.cfg.img_max, self.cfg.img_quality);
        let (tx, rx) = channel();
        let c = ctx.clone();
        std::thread::spawn(move || {
            let r = match src {
                ImageSrc::File(p) => {
                    let name = p.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                    std::fs::read(&p).map_err(|x| format!("{}: {x}", p.display())).and_then(|b| img::process(&b, &name, max, quality))
                }
                ImageSrc::Rgba { w, h, data } => img::process_rgba(&data, w, h, "붙여넣은 그림", max, quality),
            };
            let _ = tx.send(r);
            c.request_repaint();
        });
        if let Ok(mut g) = JOB.lock() {
            *g = Some(Job { rx, doc, line, join });
        }
        self.say("이미지 처리 중…");
    }

    /// 쓰지 않는 이미지를 정리한다. 편집 중인 본문이 아직 DB에 저장되기 전이면 방금 넣은 그림이 "안 쓰임"으로 보여
    /// 지워질 수 있으므로, 먼저 저장하고(저장하지 못하면 정리하지 않는다) 정리한다.
    pub(super) fn prune_images_now(&mut self) -> Result<usize, String> {
        if !self.flush() {
            return Err("편집 중인 내용을 저장하지 못해 정리를 건너뜁니다".into());
        }
        self.db.prune_images()
    }

    /// 메뉴 "이미지 삽입…": 파일을 골라 넣는다(여러 장 가능).
    pub(super) fn insert_image_dialog(&mut self) {
        let Some(ctx) = CTX.lock().ok().and_then(|g| g.clone()) else { return };
        if let Some(files) = rfd::FileDialog::new().add_filter("이미지", &["png", "jpg", "jpeg", "bmp", "gif", "webp"]).pick_files() {
            for p in files {
                self.start_image_job(&ctx, ImageSrc::File(p));
            }
        }
    }

    /// 끝난 이미지 작업을 본문에 넣고, 기다리는 파일이 있으면 다음 작업을 시작한다.
    pub(super) fn poll_image(&mut self, ctx: &egui::Context) {
        Self::remember_ctx(ctx);
        let mut crashed = false;
        let done = {
            let Ok(mut g) = JOB.lock() else { return };
            match g.as_ref().map(|j| (j.rx.try_recv(), j.doc, j.line, j.join)) {
                Some((Ok(r), doc, line, join)) => {
                    *g = None;
                    Some((r, doc, line, join))
                }
                Some((Err(TryRecvError::Disconnected), ..)) => {
                    *g = None;
                    crashed = true;
                    None
                }
                _ => None,
            }
        };
        if crashed {
            self.say("이미지를 넣지 못했습니다: 내부 오류로 작업이 중단되었습니다");
        }
        if let Some((r, doc, line, join)) = done {
            match r {
                Ok(p) => self.finish_image(p, doc, line, join),
                Err(x) => self.say(&format!("이미지를 넣지 못했습니다: {x}")),
            }
        }
        if !Self::image_busy() {
            let next = QUEUE.lock().ok().and_then(|mut q| q.pop_front());
            match next {
                Some(p) => self.start_image_job(ctx, ImageSrc::File(p)),
                None => {
                    if let Ok(mut n) = NEXT_LINE.lock() {
                        *n = None; // 묶음이 끝났다
                    }
                }
            }
        }
    }

    fn finish_image(&mut self, p: Processed, doc: i64, line: Option<usize>, join: bool) {
        if self.cur != Some(doc) || self.cur_trashed {
            self.say("문서를 바꿔서 이미지 삽입을 취소했습니다");
            return;
        }
        if let Err(x) = self.db.put_image(&p.hash, &p.webp, p.w, p.h) {
            self.say(&format!("이미지를 저장하지 못했습니다: {x}"));
            return;
        }
        let text = md::format_image_line(&p.alt, &p.hash, p.w, p.h);
        // 커서가 이미지 줄 위에 있으면 그 줄 뒤에 이어 붙여 한 줄에 여러 장이 되게 한다.
        let joined = if join { line.and_then(|l| md::append_to_image_row(&self.body, l, &text).map(|b| (l, b))) } else { None };
        let idx = if let Some((l, b)) = joined {
            self.body = b;
            Some(l)
        } else {
            self.body = md::insert_block_line(&self.body, line.unwrap_or(usize::MAX), &text);
            match line {
                Some(l) => line_index_of(&self.body, &text, l),
                None => self.body.split('\n').enumerate().filter(|(_, l)| l.trim() == text).map(|(i, _)| i).last(), // 끝에 붙은 줄
            }
        };
        self.last_edit.get_or_insert_with(Instant::now);
        if let Some(i) = idx {
            if let Ok(mut n) = NEXT_LINE.lock() {
                *n = Some((doc, i));
            }
        }
        self.say(&format!("이미지를 넣었습니다 ({}×{}, {}KB)", p.w, p.h, (p.webp.len() + 1023) / 1024));
    }

    /// 편집창에 포커스가 있을 때 Ctrl+V로 그림이 붙여넣어졌으면(`PasteImage`) 이미지로 넣는다. 글 붙여넣기는 egui가 한다.
    pub(super) fn paste_image_if_any(&mut self, ctx: &egui::Context) {
        let Some(im) = ctx.input(|i| find_paste_image(&i.events)) else { return };
        let (w, h, data) = color_image_to_rgba(&im);
        self.start_image_job(ctx, ImageSrc::Rgba { w, h, data });
    }

    /// 옵션 창의 "이미지" 구역.
    pub(super) fn image_options(&mut self, ui: &mut egui::Ui, notes: &mut Vec<String>) {
        ui.separator();
        ui.label(egui::RichText::new("이미지").strong());
        ui.horizontal_wrapped(|ui| {
            ui.label("줄일 때 긴 변 최대");
            egui::ComboBox::from_id_salt("img_max").selected_text(format!("{}px", self.cfg.img_max)).show_ui(ui, |ui| {
                for v in [800u32, 1200, 1600, 2400] {
                    if ui.selectable_value(&mut self.cfg.img_max, v, format!("{v}px")).changed() {
                        self.graph_cfg_dirty = true;
                    }
                }
            });
            ui.label("품질");
            if ui.add(egui::Slider::new(&mut self.cfg.img_quality, 60..=95)).changed() {
                self.graph_cfg_dirty = true; // 슬라이더를 놓은 뒤 저장
            }
        });
        let stale = STATS.lock().ok().map_or(true, |g| g.as_ref().map_or(true, |s| s.0.elapsed() > Duration::from_secs(2)));
        if stale {
            if let (Ok((n, b)), Ok(mut g)) = (self.db.image_stats(), STATS.lock()) {
                *g = Some((Instant::now(), n, b));
            }
        }
        let (n, bytes) = STATS.lock().ok().and_then(|g| g.as_ref().map(|s| (s.1, s.2))).unwrap_or((0, 0));
        ui.horizontal_wrapped(|ui| {
            ui.label(format!("저장된 이미지 {n}개 · {:.1}MB", bytes as f64 / 1_048_576.0));
            if ui.button("사용하지 않는 이미지 정리").on_hover_text("어느 문서(휴지통 포함)에서도 쓰지 않는 이미지를 지웁니다").clicked() {
                match self.prune_images_now() {
                    Ok(k) => notes.push(format!("사용하지 않는 이미지 {k}개를 지웠습니다")),
                    Err(x) => notes.push(format!("이미지 정리 실패: {x}")),
                }
                if let Ok(mut g) = STATS.lock() {
                    *g = None;
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn is_image_path_cases() {
        for p in ["a.PNG", "b.jpeg", "c.webp", "d.jpg", "e.bmp", "f.gif"] {
            assert!(is_image_path(Path::new(p)), "{p}");
        }
        for p in ["d.md", "e.txt", "noext", "x.png.md"] {
            assert!(!is_image_path(Path::new(p)), "{p}");
        }
    }

    #[test]
    fn config_defaults_when_missing() {
        let c: Config = serde_json::from_str("{}").unwrap();
        assert_eq!((c.img_max, c.img_quality), (1600, 80));
        let c: Config = serde_json::from_str(r#"{"img_max": 800, "img_quality": 65}"#).unwrap();
        assert_eq!((c.img_max, c.img_quality), (800, 65));
    }

    #[test]
    fn color_image_converts_to_unmultiplied_rgba() {
        let px = vec![egui::Color32::from_rgba_unmultiplied(10, 20, 30, 255), egui::Color32::from_rgba_unmultiplied(200, 100, 50, 128)];
        let (w, h, d) = color_image_to_rgba(&egui::ColorImage::new([2, 1], px));
        assert_eq!((w, h, d.len()), (2, 1, 8));
        assert_eq!(&d[..4], &[10, 20, 30, 255]);
        assert_eq!(d[7], 128);
        for (got, want) in d[4..7].iter().zip([200u8, 100, 50]) {
            assert!(got.abs_diff(want) <= 2, "{got} vs {want}"); // 곱해진 알파를 되돌릴 때 생기는 반올림 오차
        }
    }

    #[test]
    fn paste_image_event_is_found() {
        let im = std::sync::Arc::new(egui::ColorImage::new([1, 1], vec![egui::Color32::WHITE]));
        let evs = vec![egui::Event::Paste("글".into()), egui::Event::PasteImage(im.clone())];
        assert!(find_paste_image(&evs).is_some());
        assert!(find_paste_image(&[egui::Event::Paste("글".into())]).is_none());
    }

    #[test]
    fn line_index_of_finds_inserted_line() {
        let body = "a\n\n![x](img:0123456789abcdef 8x6)\n\nb";
        assert_eq!(line_index_of(body, "![x](img:0123456789abcdef 8x6)", 0), Some(2));
        assert_eq!(line_index_of(body, "![x](img:0123456789abcdef 8x6)", 3), None);
    }
}
