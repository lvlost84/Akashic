# 아카식 (Akashic)

문서를 분류와 태그로 모아 두고 빠르게 편집하는 오프라인 마크다운 노트. Rust + eframe/egui + SQLite.

## 설치
[Releases](../../releases)에서 `Akashic.exe`를 받아 실행합니다(설치 과정 없음). 이후에는 앱이 새 버전을 확인해 자동으로 내려받아 설치하고, 다시 시작하면 적용됩니다(옵션 > 정보에서 끌 수 있음).

## 빌드
```
cargo build --release     # target\release\akashic.exe
cargo test --release
```
릴리스 빌드는 `AKASHIC_REPO=사용자/저장소` 환경변수가 설정된 경우에만 업데이트 확인이 켜집니다(GitHub Actions가 자동으로 넣음).

## 릴리스 올리기
1. `Cargo.toml`의 `version`을 올린다(예: 2.0.1).
2. `git tag v2.0.1 && git push origin v2.0.1` — Actions가 빌드해 `Akashic.exe`, `SHA256SUMS`를 Releases에 올린다.

개발 기록: [DEVLOG.md](DEVLOG.md)
