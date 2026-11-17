//! 도움말 새 창(F1): 마크다운 문법(설명·입력 예시·실제 결과)과 단축키·사용법.
//! 별도의 운영체제 창으로 열리므로 편집 중인 창 옆에 두고 볼 수 있다.
use super::*;

#[derive(Clone, Copy, PartialEq, Debug)]
enum Kind {
    /// 설명만
    Plain,
    /// 예시를 편집창과 같은 서식 엔진으로 그려 결과를 보인다
    Inline,
    Table,
    Code,
    Rule,
    /// "단축키\t설명" 줄들을 표로(## 로 시작하는 줄은 소제목)
    Keys,
}

struct Topic {
    group: &'static str,
    title: &'static str,
    desc: &'static str,
    example: &'static str,
    kind: Kind,
}

const TOPICS: &[Topic] = &[
    Topic {
        group: "기본",
        title: "시작하기",
        desc: "아카식은 문서를 분류와 태그로 모아 두고 빠르게 편집하는 오프라인 마크다운 노트입니다.\n\n• 모든 문서는 DB 파일 하나(vault.db)에 자동 저장됩니다. 정한 간격(기본 5분)마다 저장되고, 하단 상태바의 막대로 남은 시간을 볼 수 있습니다.\n• 별도의 미리보기가 없습니다. 쓰는 즉시 서식이 적용되고, 커서가 있는 줄에서만 # · ** 같은 문법 기호가 보입니다.\n• 왼쪽에서 분류·태그·검색으로 문서를 찾고, 오른쪽 사이드바에서 연결 그래프나 목차를 볼 수 있습니다.\n• 왼쪽 목록에서 문서를 끌어 분류·태그·휴지통에 놓을 수 있습니다.",
        example: "",
        kind: Kind::Plain,
    },
    Topic {
        group: "기본",
        title: "화면 구성",
        desc: "• 왼쪽: 새 문서, 검색, 보기(전체·미분류·★·휴지통), 분류 트리, 태그, 문서 목록\n• 가운데: 제목, 태그, 본문(쓰는 즉시 서식)\n• 오른쪽: 연결 그래프 ↔ 목차 (위쪽 \"연결\"·\"목차\" 버튼으로 열고 전환)\n• 아래: 상태바 — Num/Caps/Scroll 잠금 키 상태, 저장·병합 막대, DB·캐시 정보, 메모리(5분마다 갱신), 글자 배율, 백업, 동기화 상태, 글자·줄·단어 수, 커서 위치\n\n상태바의 동기화·백업·데이터 칸을 누르면 옵션의 해당 탭이 열립니다.",
        example: "",
        kind: Kind::Plain,
    },
    Topic {
        group: "마크다운 문법",
        title: "제목",
        desc: "줄 맨 앞에 # 을 1~6개 쓰면 제목입니다. # 뒤에 공백 없이 붙여 써도(###제목) 제목으로 보입니다. 제목은 오른쪽 \"목차\"에 단계별로 모입니다.\n\n주의: 다른 마크다운 프로그램은 공백이 없으면 제목으로 보지 않으므로, .md로 내보내 다른 곳에서 볼 문서는 \"### 제목\"처럼 띄어 쓰는 편이 안전합니다.",
        example: "# 큰 제목\n## 중간 제목\n### 작은 제목\n###붙여 쓴 제목",
        kind: Kind::Inline,
    },
    Topic {
        group: "마크다운 문법",
        title: "굵게 · 기울임 · 취소선",
        desc: "• **굵게**  — 앞뒤에 ** 두 개\n• *기울임*  — 앞뒤에 * 한 개 (또는 _)\n• ~~취소선~~  — 앞뒤에 ~~ 두 개\n\n편집창에서 글자를 선택하고 Ctrl+B / Ctrl+I / Ctrl+Shift+X 를 누르면 기호가 자동으로 붙습니다. snake_case_name 처럼 낱말 중간의 _ 는 기울임으로 보지 않습니다.",
        example: "**굵게** 와 *기울임* 그리고 ~~취소선~~\nsnake_case_name 은 그대로",
        kind: Kind::Inline,
    },
    Topic {
        group: "마크다운 문법",
        title: "인라인 코드",
        desc: "글 중간의 코드나 명령어는 ` (백틱) 하나로 감쌉니다. Ctrl+` 로도 감쌀 수 있습니다. 코드 안의 글자는 서식 없이 그대로 보이고 고정폭 글꼴로 나옵니다.",
        example: "설정은 `config.json` 에 저장됩니다.",
        kind: Kind::Inline,
    },
    Topic {
        group: "마크다운 문법",
        title: "코드 블록",
        desc: "``` 로 시작하는 줄과 ``` 로 끝나는 줄 사이가 코드 블록입니다. 첫 줄 뒤에 언어 이름을 쓰면 박스 오른쪽 위에 작게 표시됩니다.\n\n커서가 블록 밖에 있으면 ``` 줄은 숨겨지고 박스만 보이며, 커서가 블록 안에 들어가면 원문이 그대로 보여 고칠 수 있습니다. 닫는 ``` 가 없으면 문서 끝까지가 코드 블록입니다.",
        example: "```rust\nfn main() {\n    println!(\"안녕\");\n}\n```",
        kind: Kind::Code,
    },
    Topic {
        group: "마크다운 문법",
        title: "인용",
        desc: "줄 맨 앞에 > 를 쓰면 인용입니다. 인용 줄에서 Enter 를 누르면 다음 줄에도 > 가 이어지고, 빈 인용 줄에서 Enter 를 누르면 인용이 끝납니다.",
        example: "> 인용문은 이렇게 보입니다\n> 두 번째 줄도 인용입니다",
        kind: Kind::Inline,
    },
    Topic {
        group: "마크다운 문법",
        title: "목록",
        desc: "줄 맨 앞에 - 또는 * 또는 + 를 쓰면 글머리 목록, 1. 처럼 숫자와 점을 쓰면 번호 목록입니다. 앞에 공백을 넣으면 하위 항목이 됩니다.\n\nEnter 를 누르면 다음 항목이 자동으로 이어지고(번호는 하나씩 커짐), 내용이 없는 빈 항목에서 Enter 를 누르면 목록이 끝납니다.\n\n항목을 끼워 넣거나 지우면 번호 목록의 번호가 자동으로 다시 매겨집니다(첫 항목 번호부터, 실행 취소로 되돌릴 수 있음). 옵션 > 화면·편집에서 끌 수 있습니다.",
        example: "- 사과\n- 배\n  - 하위 항목\n1. 첫째\n2. 둘째",
        kind: Kind::Inline,
    },
    Topic {
        group: "마크다운 문법",
        title: "체크박스",
        desc: "목록 항목 앞에 [ ] 또는 [x] 를 쓰면 체크박스입니다. 대괄호 부분을 클릭하면 바로 켜지고 꺼지며, 끝낸 항목은 글에 취소선과 흐린 색이 적용됩니다.\n\n코드 블록 안이나 목록 기호 없이 혼자 쓴 [ ] 는 체크박스로 동작하지 않습니다.",
        example: "- [ ] 해야 할 일\n- [x] 끝낸 일",
        kind: Kind::Inline,
    },
    Topic {
        group: "마크다운 문법",
        title: "링크",
        desc: "[보일 글](주소) 형태로 씁니다. Ctrl+K 를 누르면 선택한 글자에 링크 모양이 붙습니다. 링크 글자는 밑줄과 링크 색으로 보입니다.\n\n현재 버전은 링크를 표시만 하고, 클릭해서 브라우저로 여는 기능은 아직 없습니다.",
        example: "[아카식 홈](https://example.com) 으로 이동",
        kind: Kind::Inline,
    },
    Topic {
        group: "마크다운 문법",
        title: "문서 링크 [[ ]]",
        desc: "다른 문서를 가리킬 때는 [[문서 제목]] 또는 [[문서 제목|보일 글]] 로 씁니다. Ctrl+클릭하면 그 제목의 문서로 이동하고, 그런 문서가 없으면 지금 문서와 같은 분류에 새로 만듭니다. 제목 비교는 대소문자를 무시합니다.\n\n• Alt+← / Alt+→ 로 방금 보던 문서로 돌아가거나 다시 앞으로 갑니다.\n• 문서 링크는 오른쪽 \"연결\" 그래프와 백링크(이 문서를 가리키는 문서) 목록에 나타납니다. 아직 없는 문서는 점선 원입니다.",
        example: "자세한 내용은 [[측정기 점검]] 문서 또는\n[[측정기 점검|점검 기록]] 을 보세요.",
        kind: Kind::Inline,
    },
    Topic {
        group: "마크다운 문법",
        title: "표",
        desc: "| 로 칸을 나누고, 바로 아래 줄에 구분선(|---|---|)을 쓰면 표입니다. 구분선에 :--- (왼쪽) ---: (오른쪽) :---: (가운데) 를 쓰면 정렬이 됩니다. 칸 안에서 | 를 쓰려면 \\| 로 씁니다. 칸 안에서 줄을 바꾸려면 <br> 을 씁니다(<br/>, <br /> 도 됩니다. 예: 첫째 줄<br>둘째 줄). 그 칸이 있는 행은 줄 수만큼 높아집니다.\n\n커서가 표 밖에 있으면 테두리 있는 격자로, 표 안에 들어가면 원문으로 보입니다. 표 줄을 클릭하면 그 줄로 커서가 갑니다. 구분선이 없는 | 줄은 표가 아니라 고정폭 글자로만 보입니다.",
        example: "| 이름 | 수량 |\n|:---|---:|\n| 사과 | 3 |\n| 배 | 12 |",
        kind: Kind::Table,
    },
    Topic {
        group: "마크다운 문법",
        title: "가로줄",
        desc: "---  또는  ***  또는  ___  만 있는 줄은 가로줄입니다. 커서가 그 줄 밖에 있으면 줄 폭 전체의 가로선으로, 커서가 그 줄에 있으면 원문으로 보입니다.",
        example: "---",
        kind: Kind::Rule,
    },
    Topic {
        group: "마크다운 문법",
        title: "이스케이프 (\\)",
        desc: "서식 기호를 글자 그대로 쓰고 싶으면 앞에 \\ 를 붙입니다. \\* \\_ \\` \\~ \\[ \\] \\( \\) \\# \\\\ 에 쓸 수 있고, 백슬래시는 숨고 뒤 글자만 서식 없이 보입니다. 코드 안에서는 필요하지 않습니다.",
        example: "\\*별표\\* 는 기울임이 아니라 별표 그대로",
        kind: Kind::Inline,
    },
    Topic {
        group: "마크다운 문법",
        title: "이미지",
        desc: "![설명](주소) 형태는 아직 이미지로 그려지지 않고 글자 그대로 보입니다. 이미지 표시는 앞으로 추가할 예정입니다.",
        example: "",
        kind: Kind::Plain,
    },
    Topic {
        group: "단축키",
        title: "단축키 — 문서·화면",
        desc: "",
        example: "Ctrl+N\t새 문서\nCtrl+E\t검색 칸으로 이동\nCtrl+F\t이 문서 안에서 찾기 (Enter/F3 다음, Shift+Enter/Shift+F3 이전, Esc 닫기)\nCtrl+S\t바로 저장 (평소에는 자동 저장)\nCtrl+Shift+D\t즐겨찾기(★) 켜기/끄기\nAlt+←  /  Alt+→\t방금 보던 문서로 뒤로 / 앞으로\nCtrl+\\\t왼쪽 목록 접기/펴기\nCtrl+,\t옵션 열기\nCtrl+Alt+D\t라이트 ↔ 다크 빠른 전환\nF1\t이 도움말 열기\nEsc\t도움말 창 닫기",
        kind: Kind::Keys,
    },
    Topic {
        group: "단축키",
        title: "단축키 — 편집창",
        desc: "",
        example: "Ctrl+B\t선택한 글자를 굵게 (선택이 없으면 ** ** 삽입)\nCtrl+I\t기울임\nCtrl+Shift+X\t취소선\nCtrl+`\t인라인 코드\nCtrl+K\t링크 모양 [글자](주소) 삽입\nEnter\t목록·인용·체크박스 이어쓰기 / 빈 항목에서 목록 끝내기\nCtrl+Z\t실행 취소\nCtrl+Y  /  Ctrl+Shift+Z\t다시 실행\nCtrl+A / C / X / V\t전체 선택 / 복사 / 잘라내기 / 붙여넣기\n## 마우스\n체크박스 클릭\t[ ] ↔ [x] 켜고 끄기\nCtrl+클릭\t[[문서 링크]] 로 이동\nCtrl+마우스 휠\t본문 글자 확대·축소 (상태바의 \"글자 N%\"를 누르면 100%)\nCtrl +  /  Ctrl -  /  Ctrl 0\t화면 전체 확대 · 축소 · 원래대로",
        kind: Kind::Keys,
    },
    Topic {
        group: "사용법",
        title: "끌어 놓기 · 마우스",
        desc: "• 문서 → 분류: 목록의 문서를 왼쪽 분류에 놓으면 그 분류로 이동\n• 문서 → \"미분류\": 분류 해제  /  문서 → 🗑: 휴지통으로\n• 문서 → 태그: 그 태그가 달림\n• 분류 → 다른 분류: 하위 분류가 됨  /  \"분류\" 머리글에 놓으면 최상위로\n• 직접 정렬: 목록 위 정렬 상자에서 \"직접 정렬\"을 고르면 문서를 끌어 순서를 바꿀 수 있음 (고정(★)된 문서는 항상 맨 위)\n• 분류 우클릭: 하위 분류 만들기, 이름 바꾸기, 최상위로 옮기기, 삭제\n• 문서 우클릭: 즐겨찾기, 분류로 이동, 휴지통(휴지통에서는 복원·영구 삭제)\n• 탐색기에서 .md / .txt 파일을 창에 끌어다 놓으면 가져오기",
        example: "",
        kind: Kind::Plain,
    },
    Topic {
        group: "사용법",
        title: "검색 · 분류 · 태그",
        desc: "• 검색은 제목·본문·태그에서 글자 그대로 찾습니다. 한글은 일부만 입력해도 되고, 띄어 쓴 낱말은 모두 포함하는 문서를 찾습니다.\n• 3글자 이상 낱말은 검색 색인(FTS5)으로 빠르게, 1~2글자는 전체를 훑어 찾습니다.\n• 분류를 고르면 그 분류에 직접 속한 문서만 보입니다(하위 분류의 문서는 섞이지 않음). 분류 옆 숫자는 그 분류의 문서 수이고, 하위까지 합친 수는 마우스를 올리면 나옵니다.\n• 태그를 고르면 분류와 상관없이 그 태그가 달린 문서를 모두 보여 줍니다. 여러 태그를 고르면 모두 가진 문서만 남습니다.\n• 태그는 제목 아래 칸에 입력하고 Enter (공백·쉼표로도 나뉨) — 이미 있는 태그는 자동으로 합쳐집니다.",
        example: "",
        kind: Kind::Plain,
    },
    Topic {
        group: "그래프",
        title: "목차 · 연결 그래프",
        desc: "오른쪽 위의 \"목차\"와 \"연결\" 버튼으로 오른쪽 사이드바를 열고 닫습니다. 사이드바 위쪽 탭으로도 전환할 수 있고, 마지막에 본 모드가 기억됩니다.\n\n■ 목차\n• 본문의 제목(#)을 단계별로 보여 주고, 누르면 그 위치로 이동합니다.\n• 지금 커서가 있는 구간의 제목이 강조됩니다. \"N단계까지\"로 보일 단계를 줄일 수 있습니다.\n\n■ 연결 그래프\n• 지금 문서를 중심으로 전체 보관함이 [[문서 링크]]로 어떻게 연결돼 있는지 보여 줍니다.\n• 오른쪽 위 \"분리\" 버튼으로 그래프를 별도 창으로 띄워 크게 보고, 왼쪽 목록에서 제목을 찾아 문서를 열 수 있습니다(\"붙이기\"로 되돌림).\n• 노드는 천천히 떠다니듯 움직입니다(화면 보호기처럼). 설정 메뉴의 \"잔잔한 움직임\"으로 세기를 조절하고 0이면 멈춥니다. 창이 가려지거나 포커스를 잃으면 자동으로 멈춰 자원을 아끼며, 약 16프레임/초로만 그립니다.\n• 마우스 휠: 확대/축소 · 빈 곳 끌기: 이동 · 노드 끌기: 자리 고정 · 노드 클릭: 그 문서로 이동\n• 그래프 위에서 우클릭하면 설정 메뉴가 열립니다 — 중심에서 몇 단계까지 볼지, 연결 없는 문서·없는 문서 표시, 방향 화살표, 노드 크기·색(분류별), 선 굵기, 글자 크기, 이웃 강조, 힘(당기고 미는 정도), 제목 찾기. 바꾼 설정은 자동으로 저장됩니다.",
        example: "",
        kind: Kind::Plain,
    },
    Topic {
        group: "저장·성능",
        title: "저장 · 백업 · 동기화",
        desc: "• 자동 저장: 첫 변경 후 정한 간격(옵션, 1~20분)이 지나면, 그리고 문서를 바꿀 때·Ctrl+S·종료 때 저장됩니다. 자세한 내용은 \"저장 방식과 SSD\".\n• 휴지통: 삭제한 문서는 휴지통에 30일(옵션) 보관된 뒤 영구 삭제됩니다.\n• 자동 백업: 하루 한 번 DB 옆 backup 폴더에 복사본을 만들고 지정한 개수만 보관합니다. \"지금 백업\"으로 바로 만들 수도 있습니다.\n• 동기화 저장소: 옵션 > 백업·동기화에서 폴더를 고르면 DB를 512KB 조각으로 나눠 두고 바뀐 조각만 다시 씁니다. 이 폴더를 Synology Drive 등으로 동기화하세요. DB 폴더 자체를 동기화하면 -wal/-shm 파일 때문에 위험합니다.\n• 설정 파일: %AppData%\\Akashic\\config.json",
        example: "",
        kind: Kind::Plain,
    },
    Topic {
        group: "그래프",
        title: "그래프 배치와 모양",
        desc: "그래프에서 마우스 오른쪽 버튼 → 설정 메뉴의 \"배치\"와 \"모양\"입니다.\n\n■ 배치\n• 자유(물리): 힘으로 자연스럽게 자리 잡음(기본). 노드를 끌어 고정할 수 있습니다.\n• 원형: 연결된 문서끼리 이웃하게 원 위에 늘어놓습니다.\n• 방사형: 지금 문서를 가운데 두고 단계별 동심원으로 펼칩니다(Eades, 1992).\n• 계층형: 지금 문서에서 몇 번 건너는지에 따라 위에서 아래로 층을 나눕니다(Sugiyama 등, 1981).\n• 별자리: 서로 연결된 문서 묶음마다 허브를 중심으로 한 별자리 모양으로 놓고, 큰 묶음부터 바깥 고리에 배치합니다.\n• 해바라기: 연결이 많은 문서일수록 가운데에 오는 황금각 나선(Vogel, 1979).\n(특허 문헌은 검토하지 않았고, 위 고전 논문의 방식을 단순화해 구현했습니다.)\n\n■ 점 크기와 입체 효과\n• 모든 문서는 원으로 보이고, 연결이 많을수록 크게 보입니다(\"연결이 많을수록 크게\"). 없는 문서는 점선 원입니다.\n• \"입체 효과\" 슬라이더: 마우스를 움직이면 연결이 많은 점은 앞으로 나와 더 크게, 적은 점은 뒤로 물러나 작고 흐리게 보이며 그래프가 살짝 기울어 입체처럼 보입니다(0이면 끔). 위치 계산과 클릭·끌기는 그대로입니다.",
        example: "",
        kind: Kind::Plain,
    },
    Topic {
        group: "저장·성능",
        title: "저장 방식과 SSD",
        desc: "SSD에 같은 곳을 자주 다시 쓰면 수명과 파편화에 불리합니다. 그래서 다음처럼 저장합니다.\n\n• 입력은 메모리에 모아 두고, 첫 변경 후 정한 간격(옵션 \"자동 저장 간격\", 1~20분)이 지나면 한 번에 기록합니다.\n• 기록은 작은 쓰기 로그(WAL)에 이어 붙이기만 합니다. 본 파일(vault.db)은 그때마다 다시 쓰지 않습니다.\n• 로그는 \"로그 병합 간격\"(기본 60분)마다 본 파일에 한 번 합칩니다. 프로그램을 시작·종료할 때, 로그가 64MB를 넘을 때도 합칩니다. 옵션의 \"지금 병합\"으로 바로 합칠 수도 있습니다.\n• 문서를 옮길 때, Ctrl+S, 종료할 때는 즉시 저장하므로 평소 작업이 유실되지 않습니다.\n• 상태바의 두 막대: \"저장\"은 다음 자동 저장까지, \"병합\"은 다음 병합까지 진행률입니다. 마우스를 올리면 자세한 값이 나옵니다.\n\n주의: 프로그램이 비정상 종료되면 마지막 저장 이후(최대 설정한 간격만큼)의 입력은 잃을 수 있습니다. 중요한 글은 Ctrl+S로 저장하세요.",
        example: "",
        kind: Kind::Plain,
    },
    Topic {
        group: "저장·성능",
        title: "렌더러 · 메모리 캐시",
        desc: "■ 그래픽 렌더러 (옵션 > 화면·편집)\n• DirectX(기본): 모든 Windows에 있어 가장 안전합니다. 메모리를 400MB쯤 씁니다.\n• OpenGL: 훨씬 가볍습니다(85MB쯤). 다만 GPU 드라이버가 없는 환경(원격 데스크톱·가상머신)에서는 안 될 수 있습니다.\n• Vulkan, 자동: 그래픽 드라이버가 지원할 때만 쓰세요. 저사양 PC에서 Vulkan 로드 오류가 났다면 DirectX나 OpenGL을 고르세요.\n• 고르면 별도 프로세스로 먼저 시험해 \"사용 가능/사용 불가\"를 보여 줍니다(10초 제한). 시험에서 드라이버가 죽어도 아카식은 영향받지 않습니다. 막지는 않으니 불가여도 직접 고를 수 있습니다.\n• 바꾼 값은 프로그램을 다시 시작하면 적용됩니다. 시작할 때 첫 화면을 그리기 전에 프로그램이 죽으면, 다음 시작에서 자동으로 DirectX로 되돌리고 알려 줍니다(기록: akashic.log).\n• 시작할 때 Shift 키를 누르고 있거나 실행 옵션 --safe-mode를 주면 이번 실행만 DirectX로 시작합니다(저장된 설정은 그대로).\n• 자동 대체(시작 실패 시)는 DirectX와 OpenGL 사이에서만 이뤄지고, Vulkan은 직접 고른 경우에만 씁니다.\n• 환경변수 AKASHIC_RENDERER(directx/opengl/vulkan/auto)로 임시 지정도 가능합니다.\n\n■ 글꼴 (옵션 > 화면·편집)\n• 영문·한글 글꼴을 따로 고릅니다. 시작할 때는 고른 글꼴 파일만 읽고, 설치된 글꼴 전체를 훑지 않아 빠르고 가볍습니다.\n• 글꼴 묶음 파일(.ttc)도 되며, 어느 파일의 몇 번째 글꼴인지 기억해 둡니다.\n• 고른 글꼴이 지워졌거나 못 찾으면 기본 글꼴로 시작하고 알려 줍니다. 글꼴에 없는 글자는 맑은 고딕·이모지 글꼴이 대신 그립니다.\n\n■ 메모리 캐시 (옵션 > 화면·편집)\n• DB 페이지 캐시의 최대 크기입니다(4~1024MB). 상한일 뿐이라 DB가 작으면 실제로는 그만큼 쓰지 않습니다.\n• 문서가 많고 메모리가 넉넉한 PC는 100~200MB 이상으로 늘리면 검색·목록이 빨라질 수 있고, 메모리가 적은 PC는 줄이세요(기본 64MB).\n• 상태바의 \"DB\" 칸에 마우스를 올리면 현재 캐시 상한이 나옵니다.",
        example: "",
        kind: Kind::Plain,
    },
    Topic {
        group: "저장·성능",
        title: "동기화 저장소",
        desc: "• 옵션 > 백업·동기화에서 폴더를 고르면 DB를 512KB 조각으로 나눠 두고 바뀐 조각만 다시 씁니다. 이 폴더를 Synology Drive 같은 동기화 프로그램으로 맞추세요.\n• DB 폴더 자체를 동기화하면 -wal/-shm 파일 때문에 위험합니다. 반드시 동기화 저장소 폴더만 동기화하세요.\n• 두 PC를 번갈아 쓸 때는 한쪽에서 다 쓰고 닫은 뒤, 다른 쪽에서 \"이 폴더에서 불러오기\"를 하는 것이 안전합니다. 동시에 켜 두면 덮어쓰기 경고가 나옵니다.",
        example: "",
        kind: Kind::Plain,
    },];

fn matches_filter(t: &Topic, f: &str) -> bool {
    if f.is_empty() {
        return true;
    }
    let f = f.to_lowercase();
    t.title.to_lowercase().contains(&f) || t.desc.to_lowercase().contains(&f) || t.example.to_lowercase().contains(&f) || t.group.to_lowercase().contains(&f)
}

/// 예시 표를 (정렬, 칸들) 행으로 나눈다. 구분선 줄은 정렬 정보로만 쓰고 행에서는 뺀다.
fn parse_table(src: &str) -> (Vec<Align>, Vec<Vec<String>>) {
    let mut aligns = Vec::new();
    let mut rows = Vec::new();
    for line in src.lines() {
        // 칸 안의 \| 는 칸 경계가 아니므로 잠시 다른 글자로 바꿔 두었다가 되돌린다.
        let masked = line.trim().replace("\\|", "\u{1}");
        let cells: Vec<String> = masked.trim_matches('|').split('|').map(|c| c.trim().replace('\u{1}', "|")).collect();
        if cells.iter().all(|c| !c.is_empty() && c.chars().all(|ch| ch == '-' || ch == ':')) {
            aligns = cells
                .iter()
                .map(|c| match (c.starts_with(':'), c.ends_with(':')) {
                    (true, true) => Align::Center,
                    (false, true) => Align::Max,
                    _ => Align::Min,
                })
                .collect();
        } else {
            rows.push(cells);
        }
    }
    (aligns, rows)
}

fn show_result(ui: &mut egui::Ui, t: &Topic, theme: &Theme, size: f32) {
    let frame = egui::Frame::new().fill(theme.bg).stroke(Stroke::new(1.0, theme.border)).inner_margin(egui::Margin::same(10));
    frame.show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        match t.kind {
            Kind::Inline => {
                let job = md::build(t.example, None, size, theme, ui.available_width(), true);
                ui.label(job);
            }
            Kind::Rule => {
                ui.add_space(6.0);
                let (r, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0), Sense::hover());
                ui.painter().rect_filled(r, CornerRadius::ZERO, theme.dim);
                ui.add_space(6.0);
            }
            Kind::Code => {
                let mut lines: Vec<&str> = t.example.lines().collect();
                let lang = lines.first().and_then(|l| l.trim_start().strip_prefix("```")).unwrap_or("").trim().to_string();
                if !lines.is_empty() {
                    lines.remove(0);
                }
                if lines.last().map_or(false, |l| l.trim_start().starts_with("```")) {
                    lines.pop();
                }
                egui::Frame::new().fill(theme.code_bg).stroke(Stroke::new(1.0, theme.border)).inner_margin(egui::Margin::same(8)).show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    if !lang.is_empty() {
                        ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                            ui.label(egui::RichText::new(&lang).size(11.0).color(theme.dim));
                        });
                    }
                    ui.label(egui::RichText::new(lines.join("\n")).monospace().color(theme.code_fg).size(size * 0.94));
                });
            }
            Kind::Table => {
                let (aligns, rows) = parse_table(t.example);
                egui::Grid::new("help_table").spacing([0.0, 0.0]).show(ui, |ui| {
                    for (r, row) in rows.iter().enumerate() {
                        for (c, cell) in row.iter().enumerate() {
                            let al = aligns.get(c).copied().unwrap_or(Align::Min);
                            let fill = if r == 0 { theme.hover } else { theme.bg };
                            egui::Frame::new().fill(fill).stroke(Stroke::new(1.0, theme.border)).inner_margin(egui::Margin::symmetric(10, 4)).show(ui, |ui| {
                                ui.set_min_width(70.0);
                                ui.with_layout(Layout::top_down(al), |ui| {
                                    let txt = egui::RichText::new(cell).size(size);
                                    ui.label(if r == 0 { txt.strong() } else { txt });
                                });
                            });
                        }
                        ui.end_row();
                    }
                });
            }
            Kind::Plain | Kind::Keys => {}
        }
    });
}

fn show_keys(ui: &mut egui::Ui, t: &Topic, theme: &Theme) {
    egui::Grid::new(("help_keys", t.title)).num_columns(2).spacing([28.0, 7.0]).striped(true).show(ui, |ui| {
        for line in t.example.lines() {
            if let Some(h) = line.strip_prefix("## ") {
                ui.label(egui::RichText::new(h).strong().color(theme.accent));
                ui.label("");
            } else if let Some((k, d)) = line.split_once('\t') {
                // 키 표시는 직각 칸 안에
                egui::Frame::new().fill(theme.hover).stroke(Stroke::new(1.0, theme.border)).inner_margin(egui::Margin::symmetric(7, 2)).show(ui, |ui| {
                    ui.label(egui::RichText::new(k).monospace().size(13.0));
                });
                ui.label(d);
            }
            ui.end_row();
        }
    });
}

/// 제목으로 항목 번호를 찾는다(없으면 0).
pub(super) fn topic_index(title: &str) -> usize {
    TOPICS.iter().position(|t| t.title == title).unwrap_or(0)
}

/// 분류(탭) 이름들(등장 순서).
fn groups() -> Vec<&'static str> {
    let mut v: Vec<&'static str> = vec![];
    for t in TOPICS {
        if !v.contains(&t.group) {
            v.push(t.group);
        }
    }
    v
}

impl App {
    /// 도움말 새 창. `show_help`가 true인 동안 매 프레임 호출한다.
    pub(super) fn help_window(&mut self, ctx: &egui::Context) {
        let theme = self.theme.clone();
        let size = self.cfg.font_size;
        let mut topic = self.help_topic.min(TOPICS.len() - 1);
        let mut filter = std::mem::take(&mut self.help_filter);
        let mut close = false;
        let mut builder = egui::ViewportBuilder::default().with_title("아카식 도움말").with_inner_size([920.0, 680.0]).with_min_inner_size([560.0, 380.0]);
        if let Ok(icon) = eframe::icon_data::from_png_bytes(include_bytes!("../../assets/icon-128.png")) {
            builder = builder.with_icon(icon);
        }
        ctx.show_viewport_immediate(egui::ViewportId::from_hash_of("akashic_help"), builder, |ui, _class| {
            if ui.ctx().input(|i| i.viewport().close_requested() || i.key_pressed(Key::Escape)) {
                close = true;
            }
            // 위쪽 분류 탭: 항목이 늘어도 한 분류의 목록만 보여 한눈에 들어오게 한다.
            let gs = groups();
            let cur_group = TOPICS[topic].group;
            egui::Panel::top("help_tabs").show(ui, |ui| {
                ui.add_space(4.0);
                ui.horizontal_wrapped(|ui| {
                    for g in &gs {
                        let icon = match *g {
                            "기본" => crate::icons::Icon::Home,
                            "마크다운 문법" => crate::icons::Icon::Edit,
                            "단축키" => crate::icons::Icon::Keyboard,
                            "사용법" => crate::icons::Icon::Pointer,
                            "그래프" => crate::icons::Icon::Graph,
                            _ => crate::icons::Icon::Save,
                        };
                        let side = crate::icons::side(ui);
                        let r = ui.selectable_label(cur_group == *g && filter.trim().is_empty(), format!("      {g}"));
                        crate::icons::paint_in(ui, &r, icon, side);
                        if r.clicked() {
                            filter.clear();
                            if let Some(i) = TOPICS.iter().position(|t| t.group == *g) {
                                topic = i;
                            }
                        }
                    }
                });
                ui.add_space(2.0);
            });
            egui::Panel::left("help_nav").default_size(240.0).size_range(190.0..=380.0).resizable(true).show(ui, |ui| {
                ui.add_space(6.0);
                ui.add(egui::TextEdit::singleline(&mut filter).hint_text("도움말 찾기").desired_width(f32::INFINITY));
                ui.add_space(4.0);
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    let mut last_group = "";
                    let mut any = false;
                    for (i, t) in TOPICS.iter().enumerate() {
                        if !matches_filter(t, filter.trim()) || (filter.trim().is_empty() && t.group != cur_group) {
                            continue;
                        }
                        any = true;
                        if t.group != last_group {
                            last_group = t.group;
                            ui.add_space(6.0);
                            ui.label(egui::RichText::new(t.group).size(12.0).color(theme.dim));
                        }
                        if ui.selectable_label(topic == i, t.title).clicked() {
                            topic = i;
                        }
                    }
                    if !any {
                        ui.label(egui::RichText::new("찾는 내용이 없습니다").color(theme.dim));
                    }
                });
            });
            egui::CentralPanel::default().show(ui, |ui| {
                let t = &TOPICS[topic];
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    ui.add_space(6.0);
                    ui.label(egui::RichText::new(t.title).size(22.0).strong());
                    ui.label(egui::RichText::new(t.group).size(12.0).color(theme.dim));
                    ui.add_space(8.0);
                    if !t.desc.is_empty() {
                        ui.add(egui::Label::new(egui::RichText::new(t.desc).size(15.0)).wrap());
                        ui.add_space(10.0);
                    }
                    match t.kind {
                        Kind::Keys => show_keys(ui, t, &theme),
                        Kind::Plain => {}
                        _ => {
                            ui.label(egui::RichText::new("입력").strong());
                            egui::Frame::new().fill(theme.code_bg).stroke(Stroke::new(1.0, theme.border)).inner_margin(egui::Margin::same(10)).show(ui, |ui| {
                                ui.set_min_width(ui.available_width());
                                ui.label(egui::RichText::new(t.example).monospace().size(14.0).color(theme.code_fg));
                            });
                            if crate::icons::btn(ui, crate::icons::Icon::Copy, "예시 복사").on_hover_text("편집창에 붙여넣어 시험해 보세요").clicked() {
                                ui.ctx().copy_text(t.example.to_string());
                            }
                            ui.add_space(8.0);
                            ui.label(egui::RichText::new("결과").strong());
                            show_result(ui, t, &theme, size);
                        }
                    }
                    ui.add_space(20.0);
                });
            });
        });
        self.help_topic = topic;
        self.help_filter = filter;
        if close {
            self.show_help = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn topics_are_well_formed() {
        assert!(TOPICS.len() >= 15);
        let mut titles = std::collections::HashSet::new();
        let theme = crate::theme::themes()[0].clone();
        for t in TOPICS {
            assert!(titles.insert(t.title), "제목 중복: {}", t.title);
            assert!(!t.title.is_empty() && !t.group.is_empty());
            match t.kind {
                Kind::Plain => assert!(!t.desc.is_empty(), "{}: 설명이 있어야 함", t.title),
                Kind::Keys => {
                    for l in t.example.lines() {
                        assert!(l.starts_with("## ") || l.contains('\t'), "{}: 단축키 줄 형식 오류: {l:?}", t.title);
                    }
                }
                _ => {
                    assert!(!t.example.is_empty(), "{}: 예시 필요", t.title);
                    // 예시가 편집창 서식 엔진을 통과해도 글자가 그대로 보존되는지
                    let job = md::build(t.example, None, 15.0, &theme, 400.0, true);
                    assert_eq!(job.text, t.example, "{}", t.title);
                }
            }
        }
    }

    #[test]
    fn table_example_parses_with_alignment() {
        let t = TOPICS.iter().find(|t| t.kind == Kind::Table).unwrap();
        let (al, rows) = parse_table(t.example);
        assert_eq!(rows.len(), 3); // 구분선 줄은 행이 아님
        assert_eq!(rows[0], vec!["이름", "수량"]);
        assert_eq!(al.len(), 2);
        assert!(matches!(al[0], Align::Min) && matches!(al[1], Align::Max));
        let (_, r) = parse_table("| a \\| b | c |\n|---|:---:|\n| 1 | 2 |");
        assert_eq!(r[0][0], "a | b");
    }

    #[test]
    fn filter_finds_topics_by_any_text() {
        let t = TOPICS.iter().find(|t| t.title == "표").unwrap();
        assert!(matches_filter(t, ""));
        assert!(matches_filter(t, "정렬"));
        assert!(matches_filter(t, "격자"));
        assert!(!matches_filter(t, "존재하지않는낱말"));
        let shortcuts: Vec<_> = TOPICS.iter().filter(|t| matches_filter(t, "ctrl+b")).collect();
        assert!(!shortcuts.is_empty(), "Ctrl+B 로 단축키 항목을 찾을 수 있어야 함");
    }
}
