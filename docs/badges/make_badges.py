"""README 맨 위에 쓰는 배지(SVG)를 만든다. 문구·색을 바꾼 뒤 `python docs/badges/make_badges.py`로 다시 만든다.

외부 서비스(shields.io 등)를 쓰지 않고 이 저장소 안의 SVG 파일만 쓴다(오프라인 노트 앱이라 같은 방식으로 맞춤).
글자 폭은 어림해서 `textLength`로 고정하므로 글꼴이 달라도 상자 밖으로 넘치지 않는다.
"""
import os
from xml.sax.saxutils import escape

OUT = os.path.dirname(os.path.abspath(__file__))
FONT = "'Malgun Gothic','Apple SD Gothic Neo','Noto Sans CJK KR','Segoe UI',Verdana,sans-serif"

LABEL_BG = "#273043"   # 왼쪽(이름) 칸: 짙은 청회색
BLUE, TEAL, GREEN, PURPLE, ORANGE = "#2f6fed", "#0f8f84", "#2e9e5b", "#7a5af8", "#d9692a"


def text_width(s, size):
    """글자 폭 어림: 한글 등 넓은 글자 1em, 대문자·숫자 0.62em, 나머지 0.56em."""
    w = 0.0
    for ch in s:
        if ord(ch) > 0x2E80:
            w += size
        elif ch.isupper() or ch.isdigit():
            w += size * 0.62
        elif ch == " ":
            w += size * 0.32
        else:
            w += size * 0.56
    return w


def badge(name, label, value, color, h=22, size=11):
    pad = 9
    lw = text_width(label, size) + pad * 2 if label else 0
    vw = text_width(value, size) + pad * 2
    w = lw + vw
    parts = [
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{w:.0f}" height="{h}" viewBox="0 0 {w:.0f} {h}" role="img" aria-label="{escape(label)}: {escape(value)}">',
        f'<title>{escape(label)}: {escape(value)}</title>',
        f'<clipPath id="c"><rect width="{w:.0f}" height="{h}" rx="4"/></clipPath>',
        '<g clip-path="url(#c)">',
    ]
    if label:
        parts.append(f'<rect width="{lw:.0f}" height="{h}" fill="{LABEL_BG}"/>')
    parts.append(f'<rect x="{lw:.0f}" width="{vw:.0f}" height="{h}" fill="{color}"/>')
    parts.append('</g>')
    parts.append(f'<g fill="#fff" font-family="{FONT}" font-size="{size}" text-anchor="middle">')
    if label:
        parts.append(f'<text x="{lw / 2:.1f}" y="{h / 2 + 4:.1f}" textLength="{lw - pad * 1.2:.0f}" lengthAdjust="spacingAndGlyphs">{escape(label)}</text>')
    parts.append(f'<text x="{lw + vw / 2:.1f}" y="{h / 2 + 4:.1f}" font-weight="bold" textLength="{vw - pad * 1.2:.0f}" lengthAdjust="spacingAndGlyphs">{escape(value)}</text>')
    parts.append('</g></svg>')
    with open(os.path.join(OUT, name), "w", encoding="utf-8", newline="\n") as f:
        f.write("\n".join(parts) + "\n")


def button(name, left, right, h=34, size=13):
    """README 맨 위의 큰 다운로드 단추."""
    pad = 18
    lw = text_width(left, size) + pad * 2 + len(left) * 1.2
    rw = text_width(right, size) + pad * 2 + len(right) * 1.2
    w = lw + rw
    svg = f'''<svg xmlns="http://www.w3.org/2000/svg" width="{w:.0f}" height="{h}" viewBox="0 0 {w:.0f} {h}" role="img" aria-label="{escape(left)} {escape(right)}">
<title>{escape(left)} {escape(right)}</title>
<clipPath id="c"><rect width="{w:.0f}" height="{h}" rx="6"/></clipPath>
<g clip-path="url(#c)"><rect width="{lw:.0f}" height="{h}" fill="{LABEL_BG}"/><rect x="{lw:.0f}" width="{rw:.0f}" height="{h}" fill="{BLUE}"/></g>
<g fill="#fff" font-family="{FONT}" font-size="{size}" font-weight="bold" text-anchor="middle" letter-spacing="1">
<text x="{lw / 2:.1f}" y="{h / 2 + 5:.1f}" textLength="{lw - pad * 1.2:.0f}" lengthAdjust="spacingAndGlyphs">{escape(left)}</text>
<text x="{lw + rw / 2:.1f}" y="{h / 2 + 5:.1f}" textLength="{rw - pad * 1.2:.0f}" lengthAdjust="spacingAndGlyphs">{escape(right)}</text>
</g></svg>
'''
    with open(os.path.join(OUT, name), "w", encoding="utf-8", newline="\n") as f:
        f.write(svg)


if __name__ == "__main__":
    button("download.svg", "다운로드", "Windows .exe")
    badge("platform.svg", "운영체제", "Windows 10 · 11", BLUE)
    badge("install.svg", "설치", "필요 없음 · 단일 exe", GREEN)
    badge("storage.svg", "저장", "오프라인 · SQLite 한 파일", TEAL)
    badge("sync.svg", "동기화", "NAS WebDAV · 종단간 암호화", PURPLE)
    badge("ai.svg", "AI 연동", "MCP 서버 내장", ORANGE)
    badge("built.svg", "제작", "Rust · egui", "#8a3b12")
    badge("license.svg", "라이선스", "Apache-2.0", "#c0392b")
    print("배지 8개를 만들었습니다:", OUT)
