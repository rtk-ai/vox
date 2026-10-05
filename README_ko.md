<p align="center">
  <img src="assets/banner.png" alt="vox — Voice Command" width="600">
</p>

<h1 align="center">vox</h1>

<p align="center">
  AI 코딩 에이전트를 위한 로컬 음성 도구. 음성 합성과 음성 인식을 하나의 Rust 바이너리로 제공하며, 여러 TTS 백엔드, Whisper, MCP 서버를 포함합니다.
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
# 빠른 설치 (macOS Apple Silicon, Linux x86_64 / ARM64, WSL2)
curl -fsSL https://raw.githubusercontent.com/rtk-ai/vox/main/install.sh | sh

# Homebrew (macOS Apple Silicon, Linux)
brew install rtk-ai/tap/vox
```

GPU 빌드(Metal, CUDA), 소스 빌드, 요구 사항은 [영문 README](README.md#install)를 참고하세요. `cargo install vox`는 실행하지 마세요. crates.io에서 그 이름은 다른 프로젝트의 것입니다.

## 백엔드

| 백엔드 | 언어 | 음성 복제 | 사용 가능 환경 |
|--------|------|-----------|----------------|
| `pocket` | 영어 | 가능 (`HF_TOKEN` 필요) | 모든 플랫폼. 영어이거나 언어를 지정하지 않았을 때의 기본값 |
| `piper` | 한국어를 포함한 11개 언어 | 불가 | 모든 플랫폼. 영어 외 언어의 기본값 |
| `qwen-native` | 10개 언어 | 가능 | 모든 플랫폼. Metal 또는 CUDA 빌드에서는 GPU 사용 |
| `say` | 시스템 음성 | 불가 | macOS 전용 |
| `kokoro` | — | 불가 | `--features kokoro`로 컴파일한 빌드에만 있습니다. 배포 바이너리는 `Unknown backend: kokoro`라고 응답합니다 |

Python은 어디에도 사용되지 않습니다. 음성 인식은 Whisper이며 모든 플랫폼에서 동작합니다 (`vox hear`).

## 빠른 시작

```bash
vox "Hello, world."                     # 기본 백엔드 (pocket, 영어)
vox -l ko "안녕하세요"                   # 한국어: piper 백엔드
vox -l ko --volume 2.0 "더 크게!"        # 2배 볼륨 (범위: 0.0–5.0)
echo "파이프 텍스트" | vox -l ko          # 표준 입력에서 읽기
vox setup                               # 대화형 설정 (TUI)
```

## AI 어시스턴트 통합

하나의 명령으로 **14개 AI 도구** 설정 (Claude Code, Cursor, VS Code, Zed, Codex, Gemini, Amazon Q 등):

```bash
vox init                # MCP 서버 (기본) — 모든 도구
vox init -m cli         # CLAUDE.md + Stop 훅
vox init -m all         # 모든 모드
```

## Claude Code 플러그인: 음성 시각화

vox에는 Claude Code 플러그인이 포함되어 있습니다. vox가 말하는 동안 프롬프트 위에 실제 음성 스펙트럼을 표시합니다. Claude Code(2.1.287 이상) 세션에서 다음을 실행하세요:

```text
/plugin marketplace add rtk-ai/vox
/plugin install vox@vox
/vox-wave                          # 미리보기 및 색상 설정
```

자세한 내용은 [plugins/vox](plugins/vox/README.md)(영문)를 참고하세요.

## 음성 복제

```bash
vox clone add myvoice --audio ~/voice.wav --text "전사 텍스트"    # 오디오 파일에서
vox clone record myvoice2 --duration 10                          # 또는 마이크로 녹음
vox -b qwen-native -l ko -v myvoice "당신의 목소리로 말합니다."
```

음성 복제에는 `qwen-native`를 사용합니다. `pocket`으로 복제하려면 `HF_TOKEN`이 필요합니다.

## 데몬 (모델 상주)

```bash
vox daemon start        # 모델을 메모리에 유지
vox daemon status       # 로드된 백엔드 확인
vox daemon stop         # 중지
```

데몬은 자동으로 시작되지 않습니다.

## 문서

아래 문서는 프랑스어로 작성되어 있습니다.

| 문서 | 설명 |
|------|------|
| [아키텍처](docs/ARCHITECTURE.md) | 기술 아키텍처, 백엔드, DB 스키마, MCP 프로토콜 |
| [기능](docs/FEATURES.md) | 모든 명령 및 기능 문서 |
| [가이드](docs/GUIDE.md) | 설치, 빠른 시작, 문제 해결 |

## 라이선스

[Apache-2.0](LICENSE)
