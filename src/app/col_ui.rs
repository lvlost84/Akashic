//! "열 자르기" 대화상자: 선택한 줄(선택이 없으면 문서 전체)의 각 줄에서 글자 위치 또는 탭 칸 범위만 남기거나 지운다.
use super::*;
use crate::editkeys::{ColRange, ColUnit};

pub(super) struct ColCut {
    pub open: bool,
    pub unit: ColUnit,
    /// true: 고른 범위만 남기고 나머지를 지움 / false: 고른 범위를 지움
    pub keep: bool,
    /// 0: A~B번째, 1: 처음부터 N번째, 2: 끝에서 N번째까지
    pub kind: u8,
    pub a: String,
    pub b: String,
    pub n: String,
}

impl Default for ColCut {
    fn default() -> Self {
        ColCut { open: false, unit: ColUnit::Char, keep: true, kind: 0, a: "1".into(), b: "3".into(), n: "3".into() }
    }
}

impl ColCut {
    /// 입력칸을 범위로 바꾼다. 1 이상의 정수가 아니면 이유를 돌려준다.
    fn range(&self) -> Result<ColRange, String> {
        let num = |s: &str, what: &str| -> Result<usize, String> {
            match s.trim().parse::<usize>() {
                Ok(v) if v >= 1 => Ok(v),
                _ => Err(format!("{what}은(는) 1 이상의 숫자로 적어 주세요")),
            }
        };
        Ok(match self.kind {
            0 => ColRange::Span(num(&self.a, "시작 번호")?, num(&self.b, "끝 번호")?),
            1 => ColRange::First(num(&self.n, "개수")?),
            _ => ColRange::Last(num(&self.n, "개수")?),
        })
    }

    /// 지금 설정을 한 문장으로.
    fn summary(&self) -> String {
        let u = if self.unit == ColUnit::Char { "글자" } else { "열" };
        let what = match self.range() {
            Ok(ColRange::Span(a, b)) => format!("{}~{}번째 {u}", a.min(b), a.max(b)),
            Ok(ColRange::First(n)) => format!("처음부터 {n}번째까지 {u}"),
            Ok(ColRange::Last(n)) => format!("끝에서 {n}개 {u}"),
            Err(_) => return "숫자를 적어 주세요".into(),
        };
        format!("각 줄에서 {what}만 {}", if self.keep { "남기고 나머지를 지웁니다" } else { "지웁니다" })
    }
}

impl App {
    /// 열 자르기 창. `col.open`이 true인 동안 매 프레임 부른다.
    pub(super) fn col_window(&mut self, ctx: &egui::Context) {
        let mut open = self.col.open;
        let mut apply: Option<Result<ColRange, String>> = None;
        egui::Window::new("열 자르기").open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
            let c = &mut self.col;
            ui.horizontal(|ui| {
                ui.label("단위");
                ui.radio_value(&mut c.unit, ColUnit::Char, "글자 위치").on_hover_text("줄 맨 앞부터 센 글자 번호(공백 포함, 한글도 한 글자)");
                ui.radio_value(&mut c.unit, ColUnit::Tab, "탭으로 나뉜 열").on_hover_text("탭으로 구분된 칸 번호. 남은 칸은 탭으로 다시 이어 붙입니다");
            });
            ui.horizontal(|ui| {
                ui.label("동작");
                ui.radio_value(&mut c.keep, true, "범위만 남기기");
                ui.radio_value(&mut c.keep, false, "범위 지우기");
            });
            ui.separator();
            ui.radio_value(&mut c.kind, 0, "A번째부터 B번째까지");
            ui.add_enabled_ui(c.kind == 0, |ui| {
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut c.a).desired_width(48.0));
                    ui.label("번째부터");
                    ui.add(egui::TextEdit::singleline(&mut c.b).desired_width(48.0));
                    ui.label("번째까지");
                });
            });
            ui.radio_value(&mut c.kind, 1, "처음부터 N번째까지");
            ui.radio_value(&mut c.kind, 2, "끝에서 N번째까지(끝의 N개)");
            ui.add_enabled_ui(c.kind != 0, |ui| {
                ui.horizontal(|ui| {
                    ui.label("N =");
                    ui.add(egui::TextEdit::singleline(&mut c.n).desired_width(48.0));
                });
            });
            ui.separator();
            ui.label(egui::RichText::new(c.summary()).color(self.theme.dim));
            ui.label(egui::RichText::new("대상: 선택한 줄(선택이 없으면 문서 전체). 적용한 뒤 Ctrl+Z로 되돌릴 수 있습니다.").color(self.theme.dim));
            if ui.button("적용").clicked() {
                apply = Some(c.range());
            }
        });
        self.col.open = open;
        match apply {
            Some(Ok(r)) => {
                self.edit_sel = Some(self.last_sel); // 이 창을 누르는 동안 풀린 선택 대신 편집창의 마지막 선택을 쓴다
                self.edit_req = Some(EditReq::Cols(self.col.unit, r, self.col.keep));
            }
            Some(Err(m)) => self.say(&m),
            None => {}
        }
    }
}
