//! Windows 실행 파일에 아이콘·버전 정보를 넣는다(rc.exe로 리소스를 만들어 링크).
//! rc.exe를 못 찾으면 경고만 내고 아이콘 없이 계속 빌드한다.
fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=assets/app.rc");
    println!("cargo:rerun-if-changed=assets/icon.ico");
    #[cfg(windows)]
    embed_resources();
    license_md();
}

#[cfg(windows)]
fn find_rc() -> Option<std::path::PathBuf> {
    use std::path::PathBuf;
    // PATH에 있으면 그것을 쓴다.
    if let Some(paths) = std::env::var_os("PATH") {
        for p in std::env::split_paths(&paths) {
            let c = p.join("rc.exe");
            if c.is_file() {
                return Some(c);
            }
        }
    }
    // Windows SDK의 가장 최신 버전
    let base = std::env::var_os("ProgramFiles(x86)").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Program Files (x86)"));
    let bin = base.join("Windows Kits").join("10").join("bin");
    let mut vers: Vec<PathBuf> = std::fs::read_dir(&bin).ok()?.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
    // 글자 순서가 아니라 숫자 순서로 정렬한다(10.0.9600.0 < 10.0.19041.0 < 10.0.22621.0).
    vers.sort_by_key(|p| {
        p.file_name().map(|n| n.to_string_lossy().split('.').map(|x| x.parse::<u32>().unwrap_or(0)).collect::<Vec<u32>>()).unwrap_or_default()
    });
    for v in vers.into_iter().rev() {
        let c = v.join("x64").join("rc.exe");
        if c.is_file() {
            return Some(c);
        }
    }
    None
}

#[cfg(windows)]
fn embed_resources() {
    use std::{env, path::PathBuf, process::Command};
    let res = PathBuf::from(env::var("OUT_DIR").unwrap()).join("app.res");
    let Some(rc) = find_rc() else {
        println!("cargo:warning=rc.exe를 찾지 못해 exe 아이콘을 넣지 않습니다");
        return;
    };
    // app.rc의 버전 자리표시자를 Cargo.toml의 version으로 채워 OUT_DIR에 쓴다(exe 속성의 파일·제품 버전).
    let ver = env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.0.0".into());
    let mut n: Vec<u32> = ver.split(|c: char| !c.is_ascii_digit()).filter(|x| !x.is_empty()).map(|x| x.parse().unwrap_or(0)).collect();
    n.resize(4, 0);
    let comma = format!("{},{},{},{}", n[0], n[1], n[2], n[3]);
    let Ok(tpl) = std::fs::read_to_string("assets/app.rc") else {
        println!("cargo:warning=assets/app.rc를 읽지 못해 exe 아이콘을 넣지 않습니다");
        return;
    };
    let rc_src = PathBuf::from(env::var("OUT_DIR").unwrap()).join("app.rc");
    let _ = std::fs::write(&rc_src, tpl.replace("@VER_COMMA@", &comma).replace("@VER@", &ver));
    let icon_dir = std::fs::canonicalize("assets").unwrap_or_else(|_| PathBuf::from("assets"));
    let ok = Command::new(rc).args(["/nologo", "/i"]).arg(&icon_dir).arg("/fo").arg(&res).arg(&rc_src).status().map(|s| s.success()).unwrap_or(false);
    if ok {
        println!("cargo:rustc-link-arg-bins={}", res.display());
    } else {
        println!("cargo:warning=리소스 컴파일에 실패해 exe 아이콘을 넣지 않습니다");
    }
}
/// 실제로 exe에 링크되는 라이브러리만 모아 프로젝트 폴더의 THIRD-PARTY-LICENSES.md를 만든다.
/// 기준: cargo metadata(대상 플랫폼, 현재 feature)의 일반(normal) 의존만. 빌드 전용·시험 전용·다른 OS 전용과
/// proc-macro(와 그 아래에만 달린 크레이트)는 exe에 들어가지 않으므로 제외한다. 실패해도 빌드는 계속한다.
fn license_md() {
    use serde_json::Value;
    use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
    println!("cargo:rerun-if-changed=Cargo.lock");
    println!("cargo:rerun-if-changed=Cargo.toml");
    let run = || -> Result<String, String> {
        let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
        let mut cmd = std::process::Command::new(cargo);
        cmd.args(["metadata", "--format-version", "1", "--offline"]);
        if let Ok(t) = std::env::var("TARGET") {
            cmd.args(["--filter-platform", &t]);
        }
        let out = cmd.output().map_err(|e| e.to_string())?;
        if !out.status.success() {
            return Err(String::from_utf8_lossy(&out.stderr).into_owned());
        }
        let meta: Value = serde_json::from_slice(&out.stdout).map_err(|e| e.to_string())?;
        let pk: HashMap<&str, &Value> = meta["packages"].as_array().ok_or("packages")?.iter().filter_map(|p| Some((p["id"].as_str()?, p))).collect();
        let nodes: HashMap<&str, &Value> = meta["resolve"]["nodes"].as_array().ok_or("nodes")?.iter().filter_map(|n| Some((n["id"].as_str()?, n))).collect();
        let root = meta["resolve"]["root"].as_str().ok_or("root")?;
        let is_proc = |id: &str| pk[id]["targets"].as_array().map_or(false, |ts| ts.iter().any(|t| t["kind"].as_array().map_or(false, |k| k.iter().any(|x| x == "proc-macro"))));
        let mut seen: HashSet<&str> = HashSet::from([root]);
        let mut q: VecDeque<&str> = VecDeque::from([root]);
        while let Some(id) = q.pop_front() {
            if id != root && is_proc(id) {
                continue;
            }
            for d in nodes[id]["deps"].as_array().into_iter().flatten() {
                let normal = d["dep_kinds"].as_array().map_or(false, |ks| ks.iter().any(|k| k["kind"].is_null()));
                let Some(p) = d["pkg"].as_str() else { continue };
                if normal && seen.insert(p) {
                    q.push_back(p);
                }
            }
        }
        // (이름, 버전) -> (라이선스, 폴더)
        let mut used: BTreeMap<(String, String), (String, std::path::PathBuf)> = BTreeMap::new();
        for id in &seen {
            if *id == root || is_proc(id) {
                continue;
            }
            let p = pk[id];
            let dir = std::path::Path::new(p["manifest_path"].as_str().unwrap_or("")).parent().map(|x| x.to_path_buf()).unwrap_or_default();
            used.insert((p["name"].as_str().unwrap_or("").to_string(), p["version"].as_str().unwrap_or("").to_string()), (p["license"].as_str().unwrap_or("(표기 없음)").to_string(), dir));
        }
        let mut md = String::new();
        md.push_str("# 오픈소스 라이선스 고지\n\n아카식(Akashic)은 아래 오픈소스 구성 요소를 사용합니다. 이 파일은 빌드할 때 자동으로 만들어지며, 실제로 실행 파일에 들어가는 구성 요소만 담습니다(빌드 전용 도구·다른 운영체제용·매크로 도구는 제외).\n\n");
        md.push_str(&format!("## 구성 요소 ({}개)\n\n| 이름 | 버전 | 라이선스 |\n|---|---|---|\n", used.len()));
        for ((n, v), (l, _)) in &used {
            md.push_str(&format!("| {n} | {v} | {l} |\n"));
        }
        md.push_str("\n## 추가 고지\n\n- SQLite(rusqlite 내장 libsqlite3-sys): SQLite 자체는 퍼블릭 도메인입니다.\n- egui 계열(egui, eframe, epaint, ecolor, emath, egui-winit, egui-wgpu, egui_glow)과 accesskit 계열, profiling, spirv, clipboard-win(BSL-1.0)은 패키지에 라이선스 전문 파일이 없어 위 표의 표기만 합니다.\n- epaint_default_fonts: egui 기본 글꼴(Ubuntu-Light, Hack, NotoEmoji, emoji-icon-font)을 포함합니다. 글꼴 라이선스 전문은 아래에 있습니다.\n- Windows에 설치된 글꼴(맑은 고딕, Segoe UI, Consolas 등)은 프로그램에 포함되지 않고 실행 중에 시스템에서 불러옵니다.\n\n## 라이선스 전문\n");
        // 같은 문구는 하나로 합친다
        let mut groups: Vec<(String, Vec<String>)> = Vec::new();
        for ((n, v), (_, dir)) in &used {
            let mut files: Vec<std::path::PathBuf> = Vec::new();
            for d in [dir.clone(), dir.join("fonts")] {
                let Ok(rd) = std::fs::read_dir(&d) else { continue };
                for e in rd.flatten() {
                    let name = e.file_name().to_string_lossy().to_uppercase();
                    let root_lic = d == *dir && ["LICENSE", "LICENCE", "COPYING", "UNLICENSE", "NOTICE", "COPYRIGHT"].iter().any(|p| name.starts_with(p));
                    let font_lic = d != *dir && name.ends_with(".TXT") && (name.starts_with("OFL") || name.starts_with("UFL") || name.contains("LICENSE") || name.contains("LICENCE"));
                    if e.path().is_file() && (root_lic || font_lic) {
                        files.push(e.path());
                    }
                }
            }
            files.sort();
            for f in files {
                let Ok(raw) = std::fs::read(&f) else { continue };
                let body = String::from_utf8_lossy(&raw).replace("\r\n", "\n").trim().to_string();
                if body.is_empty() {
                    continue;
                }
                let who = format!("{n} {v}");
                match groups.iter_mut().find(|(b, _)| *b == body) {
                    Some((_, w)) => {
                        if !w.contains(&who) {
                            w.push(who);
                        }
                    }
                    None => groups.push((body, vec![who])),
                }
            }
        }
        for (body, who) in &groups {
            md.push_str(&format!("\n### 적용 대상: {}\n\n~~~~text\n{}\n~~~~\n", who.join(", "), body));
        }
        Ok(md)
    };
    match run() {
        Ok(md) => {
            let dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".into());
            let path = std::path::Path::new(&dir).join("THIRD-PARTY-LICENSES.md");
            // 내용이 같으면 다시 쓰지 않는다(수정 시각이 바뀌어 불필요한 재빌드·동기화가 생기는 것을 막는다).
            if std::fs::read_to_string(&path).map_or(true, |old| old != md) {
                if let Err(e) = std::fs::write(&path, md) {
                    println!("cargo:warning=THIRD-PARTY-LICENSES.md를 쓰지 못했습니다: {e}");
                }
            }
        }
        Err(e) => println!("cargo:warning=THIRD-PARTY-LICENSES.md를 만들지 못했습니다: {}", e.lines().next().unwrap_or("")),
    }
}