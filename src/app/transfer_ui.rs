//! 폴더 가져오기, 문서 하나를 HTML·PDF로 내보내기.
use super::*;
use std::collections::HashMap;
use std::path::Path;
use std::sync::mpsc::channel;

impl App {
    /// 폴더(Obsidian·Notion·일반 마크다운·아카식 내보내기)를 읽어 하위 폴더는 분류로, 문서는 새 문서로 가져온다.
    pub(super) fn import_dir(&mut self) {
        let Some(root) = rfd::FileDialog::new().set_title("가져올 폴더 선택").pick_folder() else { return };
        self.flush();
        let sc = crate::import::scan(&root, &|b| decode_text(b).0);
        if sc.items.is_empty() {
            self.say("가져올 .md/.txt 문서가 없습니다");
            return;
        }
        let base = match self.filter.view { View::Category(c) => Some(c), _ => None };
        let mut cats = self.db.categories().unwrap_or_default();
        let mut cache: HashMap<Vec<String>, Option<i64>> = HashMap::new();
        let mut img_cache: HashMap<PathBuf, Option<(String, u32, u32)>> = HashMap::new();
        let (mut ok, mut failed, mut imgs, mut new_cats) = (0usize, sc.failed, 0usize, 0usize);
        let mut last = None;
        let (max_side, quality) = (self.cfg.img_max, self.cfg.img_quality);
        for it in &sc.items {
            // 분류 찾기·만들기(깊이 한도를 넘는 폴더는 마지막 단계에 합친다)
            let dirs: Vec<String> = it.dirs.iter().take(crate::db::MAX_DEPTH.saturating_sub(base.map_or(0, |_| 1))).cloned().collect();
            let mut parent = base;
            for k in 0..dirs.len() {
                let key = dirs[..=k].to_vec();
                parent = match cache.get(&key) {
                    Some(c) => *c,
                    None => {
                        let found = cats.iter().find(|c| c.parent == parent && c.name.trim().to_lowercase() == dirs[k].trim().to_lowercase()).map(|c| c.id);
                        let id = match found {
                            Some(id) => Some(id),
                            None => match self.db.add_category(parent, &dirs[k]) {
                                Ok(id) => {
                                    new_cats += 1;
                                    cats.push(crate::db::Category { id, parent, name: dirs[k].clone(), count: 0 });
                                    Some(id)
                                }
                                Err(_) => parent, // 만들 수 없으면 위 단계에 둔다
                            },
                        };
                        cache.insert(key, id);
                        id
                    }
                };
            }
            let body = if it.body.contains("![") {
                let db = &self.db;
                crate::import::rewrite_image_lines(&it.body, &sc.root, &it.src_dir, &sc.images, &mut |alt, path| {
                    let r = img_cache
                        .entry(path.to_path_buf())
                        .or_insert_with(|| {
                            let bytes = std::fs::read(path).ok()?;
                            let p = crate::img::process(&bytes, alt, max_side, quality).ok()?;
                            db.put_image(&p.hash, &p.webp, p.w, p.h).ok()?;
                            Some((p.hash, p.w, p.h))
                        })
                        .clone();
                    if r.is_some() {
                        imgs += 1;
                    }
                    r
                })
            } else {
                it.body.clone()
            };
            match self.db.new_doc(parent) {
                Ok(id) => match self.db.save_doc(id, &it.title, &body) {
                    Ok(_) => {
                        ok += 1;
                        for t in &it.tags {
                            let _ = self.db.add_tag(id, t);
                        }
                        if it.pinned {
                            let _ = self.db.set_pinned(id, true);
                        }
                        last = Some(id);
                    }
                    Err(_) => {
                        failed += 1;
                        let _ = self.db.delete_empty(id);
                    }
                },
                Err(_) => failed += 1,
            }
        }
        self.list_dirty = true;
        self.refresh();
        if let Some(id) = last {
            self.open_doc(id, true);
        }
        self.say(&format!(
            "폴더에서 {ok}개 가져옴 (새 분류 {new_cats}개, 그림 {imgs}개){}",
            if failed > 0 { format!(" · {failed}개는 가져오지 못했습니다") } else { String::new() }
        ));
    }

    /// 화면과 같은 모양의 스타일(테마 색·설정한 글꼴·크기·간격)
    fn html_style(&self) -> crate::mdhtml::Style {
        let hex = |c: egui::Color32| format!("#{:02x}{:02x}{:02x}", c.r(), c.g(), c.b());
        let t = &self.theme;
        let mut fonts: Vec<String> = [&self.cfg.font_latin, &self.cfg.font_korean]
            .iter()
            .filter(|n| !n.trim().is_empty())
            .map(|n| format!("'{}'", n.replace(['\'', '"', '\\'], "")))
            .collect();
        fonts.extend(["'Malgun Gothic'".to_string(), "'Segoe UI'".to_string(), "sans-serif".to_string()]);
        crate::mdhtml::Style {
            bg: hex(t.bg),
            text: hex(t.text),
            dim: hex(t.dim),
            link: hex(t.link),
            quote: hex(t.quote),
            code_bg: hex(t.code_bg),
            code_fg: hex(t.code_fg),
            border: hex(t.border),
            font: fonts.join(","),
            size: self.cfg.font_size,
            line: self.cfg.line_spacing,
            para: self.cfg.para_spacing,
            list: self.cfg.list_spacing,
        }
    }

    /// 지금 문서의 완성된 HTML(그림은 data URI로 넣고, 수식은 화면과 같은 배치의 SVG로 그린다)
    fn current_html(&self) -> String {
        let db = &self.db;
        let size = self.cfg.font_size;
        let ctx = self.ctx_for_windows.clone();
        let math = |node: &crate::math::Node| -> Option<String> {
            let ctx = ctx.as_ref()?;
            let font = |sc: f32| egui::FontId::new(size * sc, egui::FontFamily::Proportional);
            let mut measure = |s: &str, sc: f32| ctx.fonts_mut(|f| f.layout_no_wrap(s.to_string(), font(sc), egui::Color32::WHITE)).size().x / size;
            Some(crate::math::to_svg(node, size, &mut measure))
        };
        let inner = crate::mdhtml::to_html(
            &self.body,
            &|hash| {
                let (webp, _, _) = db.get_image(hash).ok().flatten()?;
                Some(format!("data:image/webp;base64,{}", crate::mdhtml::base64(&webp)))
            },
            &math,
        );
        let title = if self.title.trim().is_empty() { "제목없음" } else { self.title.trim() };
        crate::mdhtml::page_styled(title, &inner, &self.html_style())
    }

    fn doc_file_stem(&self) -> String {
        crate::export::doc_stem(&self.title, &self.body)
    }

    pub(super) fn export_html(&mut self) {
        if self.cur.is_none() {
            return;
        }
        let Some(path) = rfd::FileDialog::new().set_title("HTML로 저장").set_file_name(format!("{}.html", self.doc_file_stem())).add_filter("HTML", &["html"]).save_file() else { return };
        match std::fs::write(&path, self.current_html()) {
            Ok(_) => self.say(&format!("HTML로 저장했습니다: {}", path.display())),
            Err(x) => self.say(&format!("HTML 저장 실패: {x}")),
        }
    }

    /// Edge(Windows에 기본 설치)를 화면 없이 실행해 PDF로 저장한다. 끝나면 `poll_pdf`가 결과를 알린다.
    pub(super) fn export_pdf(&mut self) {
        if self.cur.is_none() {
            return;
        }
        if self.pdf_rx.is_some() {
            self.say("PDF를 만드는 중입니다");
            return;
        }
        let Some(edge) = find_edge() else {
            self.say("PDF 저장에는 Microsoft Edge가 필요합니다. 대신 HTML로 저장해 브라우저에서 인쇄(PDF)하세요");
            return;
        };
        let Some(out) = rfd::FileDialog::new().set_title("PDF로 저장").set_file_name(format!("{}.pdf", self.doc_file_stem())).add_filter("PDF", &["pdf"]).save_file() else { return };
        let work = std::env::temp_dir().join(format!("akashic-pdf-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&work);
        let html = work.join("doc.html");
        if let Err(x) = std::fs::write(&html, self.current_html()) {
            self.say(&format!("PDF 준비 실패: {x}"));
            return;
        }
        let (tx, rx) = channel();
        self.pdf_rx = Some(rx);
        self.say("PDF를 만드는 중…");
        std::thread::spawn(move || {
            let msg = run_edge_pdf(&edge, &work, &html, &out);
            let _ = std::fs::remove_dir_all(&work);
            let _ = tx.send(msg);
        });
    }

    /// 백그라운드 PDF 결과 확인(매 프레임 호출)
    pub(super) fn poll_pdf(&mut self, ctx: &egui::Context) {
        let Some(rx) = &self.pdf_rx else { return };
        match rx.try_recv() {
            Ok(m) => {
                self.pdf_rx = None;
                self.say(&m);
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => ctx.request_repaint_after(Duration::from_millis(500)),
            Err(_) => {
                self.pdf_rx = None;
                self.say("PDF 만들기가 중단되었습니다");
            }
        }
    }
}

fn find_edge() -> Option<PathBuf> {
    ["ProgramFiles(x86)", "ProgramFiles", "LOCALAPPDATA"]
        .iter()
        .filter_map(|v| std::env::var_os(v))
        .map(|b| PathBuf::from(b).join("Microsoft").join("Edge").join("Application").join("msedge.exe"))
        .find(|p| p.is_file())
}

fn run_edge_pdf(edge: &Path, work: &Path, html: &Path, out: &Path) -> String {
    use std::os::windows::process::CommandExt;
    let url = format!("file:///{}", html.to_string_lossy().replace('\\', "/").replace(' ', "%20"));
    let _ = std::fs::remove_file(out);
    let child = std::process::Command::new(edge)
        .arg("--headless=new")
        .arg("--disable-gpu")
        .arg("--no-first-run")
        .arg("--no-pdf-header-footer")
        .arg(format!("--user-data-dir={}", work.join("profile").display()))
        .arg(format!("--print-to-pdf={}", out.display()))
        .arg(url)
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
        .spawn();
    let mut child = match child {
        Ok(c) => c,
        Err(x) => return format!("Edge를 실행하지 못했습니다: {x}"),
    };
    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if start.elapsed() > Duration::from_secs(60) => {
                let _ = child.kill();
                return "PDF 만들기 시간 초과(60초)".into();
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(200)),
            Err(x) => return format!("Edge 상태 확인 실패: {x}"),
        }
    }
    match std::fs::metadata(out) {
        Ok(m) if m.len() > 0 => format!("PDF로 저장했습니다: {}", out.display()),
        _ => "PDF가 만들어지지 않았습니다. HTML로 저장해 브라우저에서 인쇄(PDF)해 보세요".into(),
    }
}
