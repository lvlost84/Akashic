# 아카식 (Akashic)

문서를 분류와 태그로 모아 두고 빠르게 편집하는 **오프라인 마크다운 노트**입니다. 미리보기 없이 쓰는 즉시 서식이 적용되고, 모든 문서는 SQLite 파일 하나에 저장됩니다.

<p align="center">
  <a href="../../releases/latest"><img src="docs/badges/download.svg" alt="다운로드 Windows .exe"></a>
</p>
<p align="center">
  <img src="docs/badges/platform.svg" alt="운영체제: Windows 10 · 11">
  <img src="docs/badges/install.svg" alt="설치: 필요 없음 · 단일 exe">
  <img src="docs/badges/storage.svg" alt="저장: 오프라인 · SQLite 한 파일">
  <img src="docs/badges/sync.svg" alt="동기화: NAS WebDAV · 종단간 암호화">
  <img src="docs/badges/ai.svg" alt="AI 연동: MCP 서버 내장">
  <img src="docs/badges/built.svg" alt="제작: Rust · egui">
  <a href="LICENSE"><img src="docs/badges/license.svg" alt="라이선스: Apache-2.0"></a>
</p>

## 주요 기능
- **실시간 서식 편집**: 제목·굵게·목록·표·코드 블록·체크박스 등이 입력하는 즉시 그 모양으로 보입니다.
- **분류 트리 + 태그**: 분류는 최대 6단계, 태그는 여러 개. 끌어 놓기로 정리합니다.
- **수식·이미지**: `$$…$$` 수식을 그리고, 붙여넣은 그림은 WebP로 줄여 DB에 저장합니다.
- **문서 링크**: `[[문서 제목]]`으로 잇고 연결 그래프·목차 사이드바로 봅니다. 문서를 별도 참고 창에 띄울 수 있습니다.
- **인덱스 문서(문서 지도)**: 허브 문서를 지정해 두면 새 문서에 제목이 생길 때 분류·태그에 맞는 섹션에 `[[링크]]`를 자동으로 덧붙이고, "인덱스 정리…"로 빠진 문서 추가·끊긴 링크 삭제를 골라서 처리합니다.
- **검색**: 제목·본문·태그 전체 검색, 문서 안 찾기·찾아 바꾸기.
- **편집 도구**: 우클릭 메뉴, 줄 이동·복제·삭제, 공백을 탭으로/공백 제거, 열 자르기(A~B번째·처음 N개·끝 N개 남기거나 지우기).
- **안전장치**: 자동 저장(기본 2분), 하루 한 번 자동 백업, 휴지통(기본 30일).
- **MCP 서버(AI 연동)**: Claude Code 같은 MCP 클라이언트가 문서를 검색·읽고(옵션으로 쓰기도) 할 수 있습니다. `Akashic.exe --mcp`(표준 입출력) 또는 앱 실행 중 로컬 HTTP 서버. 암호화한 글은 "암호화됨"으로 가립니다. Claude Desktop은 `.mcpb` 번들(`mcpb/pack.ps1`로 생성)을 더블클릭해 설치합니다. 사용법은 앱 도움말(F1) "AI 연동".
- **동기화**: 폴더 또는 내 NAS의 WebDAV 서버에 종단간 암호화로 동기화합니다. 스냅숏 방식과, 두 PC에서 고친 내용을 문서 단위로 합치는 방식(충돌 시 사본을 남김) 중에서 고릅니다. 설정은 [webdav-서버설정.md](docs/webdav-서버설정.md).
- **꾸미기**: 라이트·다크·레이저·사용자 지정 테마, 글꼴·간격·글자 크기.
- **자동 업데이트**: 새 버전을 확인해 내려받고, SHA256이 맞을 때만 설치합니다.

단축키와 문법은 앱의 도움말(F1)을 보세요.

## 설치
[Releases](../../releases)에서 `Akashic.exe`를 받아 실행합니다. 이후에는 앱이 하루 한 번 새 버전을 확인해 자동으로 설치합니다(옵션 > 정보에서 끌 수 있음).

## 저장 위치
- 문서: `%AppData%\Akashic\vault.db` (옵션에서 변경). 표준 SQLite 파일이라 DB Browser for SQLite 등으로 열어 볼 수 있습니다.
- 설정 `config.json`, 자동 백업 `backup\`, 오류 기록 `akashic.log`는 같은 폴더에 있습니다.
- DB 폴더 자체를 클라우드 드라이브로 동기화하지 마세요. PC 간 이동은 옵션의 **동기화 저장소**를 쓰세요.
- DB 형식을 올리는 버전은 처음 실행할 때 백업을 만든 뒤 변환하며, 변환한 DB는 이전 버전에서 열리지 않습니다.

## 빌드·릴리스
```
cargo build --release     # target\release\akashic.exe
cargo test --release
```
릴리스는 `Cargo.toml`의 `version`을 올리고 커밋한 뒤 `git tag v2.12.0` → `git push origin main v2.12.0`을 하면 GitHub Actions가 시험·빌드를 거쳐 `Akashic.exe`와 `SHA256SUMS`를 Releases에 올립니다. 업데이트 확인은 `AKASHIC_REPO` 환경변수가 설정된 빌드에서만 켜집니다.

## 라이선스
아카식은 **Apache License 2.0**입니다. 복사·수정·재배포·상업적 이용이 모두 가능하며, 재배포할 때 라이선스 전문과 고지를 함께 두어야 합니다. 보증은 없습니다(AS IS).

| 파일 | 내용 |
|---|---|
| [LICENSE](LICENSE) | 아카식의 라이선스 전문 (Apache-2.0) |
| [THIRD-PARTY-LICENSES.md](THIRD-PARTY-LICENSES.md) | 사용한 오픈소스 약 200개의 이름·버전·라이선스와 전문 (모두 MIT·Apache-2.0 등 허용적 라이선스) |

`THIRD-PARTY-LICENSES.md`는 빌드할 때 자동으로 만들어지며, 의존 라이브러리가 바뀌면 함께 갱신해 저장소에 올립니다.

## 기록
개발 기록은 [DEVLOG.md](DEVLOG.md)에 있습니다.
