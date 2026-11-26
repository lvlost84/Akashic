# Gitea Actions 설정 (Windows 러너)

`.gitea/workflows/release.yml`이 태그(`v*`)를 올리면 시험·빌드 후 Gitea 릴리스에 `Akashic.exe`, `SHA256SUMS`, `THIRD-PARTY-LICENSES.md`, `.mcpb`를 올립니다. `main` 푸시(문서만 바뀐 경우 제외)에는 시험·빌드만 합니다. Gitea는 `.gitea/workflows`가 있으면 `.github/workflows`를 읽지 않으므로 GitHub 판은 그대로 둡니다.

## 1. Gitea 서버
1. Actions 사용: Gitea 1.21 이상은 기본으로 켜져 있습니다. 더 낮으면 `app.ini`에 `[actions]` / `ENABLED = true`를 넣고 재시작합니다.
2. 저장소 → 설정 → 고급 설정 → **저장소 Actions 활성화**를 체크합니다.

## 2. 러너 PC (Windows)
빌드는 이 PC에서 실행됩니다. 아래가 설치돼 있어야 합니다.
- Rust(MSVC 툴체인), Visual Studio Build Tools(C++), Windows SDK(`rc.exe`: exe 아이콘·버전 정보에 필요), Git

러너 설치·등록:
1. Gitea의 `act_runner` Windows 버전(https://gitea.com/gitea/act_runner/releases)을 받아 폴더(예: `C:\act_runner`)에 둡니다.
2. 저장소 → 설정 → Actions → Runners → **러너 등록**에서 등록 토큰을 확인합니다(토큰은 다른 사람에게 보이지 않게 하세요).
3. PowerShell에서 등록합니다. 라벨 `windows:host`가 "이 PC에서 직접 실행"이라는 뜻이고, 워크플로의 `runs-on: windows`와 맞아야 합니다.
   ```powershell
   .\act_runner.exe register --no-interactive --instance https://gitea.canal.synology.me --token <등록토큰> --name windows-pc --labels windows:host
   ```
4. 실행합니다: `.\act_runner.exe daemon` (계속 켜 두거나 작업 스케줄러/NSSM으로 서비스 등록).
5. Gitea 러너 목록에서 상태가 "대기 중(Idle)"이면 됩니다.

## 3. 토큰 권한
릴리스 생성은 실행마다 자동 발급되는 토큰(`github.token`)으로 시도합니다. 릴리스 단계가 403으로 실패하면 개인 액세스 토큰(권한: repository 읽기·쓰기)을 만들어 저장소 → 설정 → Actions → Secrets에 `RELEASE_TOKEN`으로 등록하세요. 워크플로가 그 값을 우선 씁니다.

## 4. 캐시
빌드 결과를 `C:\act-cache\akashic-target`에 남겨 다음 빌드부터 빨라집니다(첫 빌드는 약 4분). 폴더를 바꾸려면 워크플로의 `CARGO_TARGET_DIR`을 고칩니다.

## 5. 알아 둘 점
- 앱의 자동 업데이트(`src/update.rs`)는 GitHub 릴리스 API를 봅니다. Gitea에만 릴리스하면 앱이 새 버전을 찾지 못하므로, Gitea 주소를 보게 하려면 별도 수정이 필요합니다.
- 러너 PC가 꺼져 있으면 빌드가 대기합니다.
