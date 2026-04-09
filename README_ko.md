<p align="center">
  <img src="assets/banner.png" alt="vox — Voice Command" width="600">
</p>

<h1 align="center">vox</h1>

<p align="center">
  5개의 백엔드와 AI 어시스턴트용 MCP 서버를 갖춘 크로스플랫폼 TTS CLI 도구.
</p>

<p align="center">
  <a href="README.md">English</a> &bull;
  <a href="README_fr.md">Fran&ccedil;ais</a> &bull;
  <a href="README_zh.md">中文</a> &bull;
  <a href="README_ja.md">日本語</a> &bull;
  <a href="README_ko.md">한국어</a> &bull;
  <a href="README_es.md">Espa&ntilde;ol</a>
</p>

---

## 설치

```bash
# 빠른 설치 (macOS / Linux / WSL)
curl -fsSL https://raw.githubusercontent.com/rtk-ai/vox/main/install.sh | sh

# 소스에서 설치
cargo install --path .

# GPU 가속
cargo install --path . --features metal  # macOS Apple Silicon
cargo install --path . --features cuda   # Linux NVIDIA
```

## 빠른 시작

```bash
vox "Hello, world."                     # 기본 백엔드로 읽기
vox -b voxtream "Zero-shot TTS."        # VoXtream2 (가장 빠름)
vox -b kokoro -l ko "안녕하세요"         # Kokoro 한국어
vox --volume 2.0 "더 크게!"            # 2배 볼륨 (범위: 0.0–5.0)
echo "파이프 텍스트" | vox               # 표준 입력에서 읽기
vox setup                               # 대화형 설정 (TUI)
```

## AI 어시스턴트 통합

하나의 명령으로 **14개 AI 도구** 설정 (Claude Code, Cursor, VS Code, Zed, Codex, Gemini, Amazon Q 등):

```bash
vox init                # MCP 서버 (기본) — 모든 도구
vox init -m cli         # CLAUDE.md + Stop 훅
vox init -m all         # 모든 모드
```

## 음성 복제

```bash
vox clone add myvoice --audio ~/voice.wav --text "전사 텍스트"
vox clone record myvoice --duration 10
vox -v myvoice "당신의 목소리로 말합니다."
```

## 데몬 (모델 상주)

```bash
vox daemon start        # 모델을 메모리에 유지
vox daemon status       # 로드된 백엔드 확인
vox daemon stop         # 중지
```

## 문서

| 문서 | 설명 |
|------|------|
| [아키텍처](docs/ARCHITECTURE.md) | 기술 아키텍처, 백엔드, DB 스키마, MCP 프로토콜 |
| [기능](docs/FEATURES.md) | 모든 명령 및 기능 문서 |
| [가이드](docs/GUIDE.md) | 설치, 빠른 시작, 문제 해결 |

## 라이선스

[Apache-2.0](LICENSE)
