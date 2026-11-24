//! 인터넷 링크: Ctrl+클릭이나 우클릭 메뉴로 기본 브라우저(mailto는 메일 앱)에서 연다.
use super::*;

impl App {
    /// 주소를 기본 프로그램으로 연다. `http(s)`·`mailto`만 열고, 셸을 거치지 않는다.
    pub(super) fn open_url(&mut self, raw: &str) {
        let Some(url) = crate::md::safe_url(raw) else {
            self.say("열 수 없는 주소입니다(http·https·mailto·www만 열 수 있습니다)");
            return;
        };
        match std::process::Command::new("rundll32.exe").arg("url.dll,FileProtocolHandler").arg(&url).spawn() {
            Ok(_) => self.say(&format!("브라우저로 엽니다: {}", short_url(&url, 60))),
            Err(x) => self.say(&format!("주소를 열 수 없습니다: {x}")),
        }
    }
}

impl App {
    /// 경로의 파일을 탐색기에서 선택해 보여 준다(파일은 실행하지 않는다). 파일이 없으면 있는 가장 가까운 폴더를 연다.
    pub(super) fn show_in_folder(&mut self, raw: &str) {
        use std::os::windows::process::CommandExt;
        let Some(path) = crate::md::local_path(raw) else {
            self.say("열 수 없는 경로입니다");
            return;
        };
        let unc = path.starts_with("\\\\");
        let p = std::path::Path::new(&path);
        // 공유 폴더는 응답이 늦을 수 있어 확인 없이 탐색기에 맡긴다.
        let (target, select) = if unc || p.exists() {
            (path.clone(), true)
        } else {
            match p.ancestors().skip(1).find(|a| a.exists()) {
                Some(a) => (a.display().to_string(), false),
                None => {
                    self.say(&format!("경로를 찾을 수 없습니다: {}", short_url(&path, 60)));
                    return;
                }
            }
        };
        let mut cmd = std::process::Command::new("explorer");
        if select {
            cmd.raw_arg(format!("/select,\"{target}\""));
        } else {
            cmd.arg(&target);
        }
        match cmd.spawn() {
            Ok(_) if select => self.say(&format!("폴더에서 봅니다: {}", short_url(&path, 60))),
            Ok(_) => self.say("그 경로가 없어 있는 가장 가까운 폴더를 열었습니다"),
            Err(x) => self.say(&format!("탐색기를 열 수 없습니다: {x}")),
        }
    }
}

/// 메뉴·안내에 보일 짧은 주소(글자 수 기준으로 자른다).
pub(super) fn short_url(u: &str, max: usize) -> String {
    if u.chars().count() <= max {
        u.to_string()
    } else {
        format!("{}…", u.chars().take(max).collect::<String>())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_url_cuts_by_chars() {
        assert_eq!(short_url("https://a.b", 60), "https://a.b");
        assert_eq!(short_url("가나다라마", 3), "가나다…");
    }
}
