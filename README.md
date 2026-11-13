<div align="center">
  <img src="src/assets/ttobak-logo.png" alt="또박 로고" width="96" height="96" />
  <h1>또박 (Ttobak)</h1>
  <p>
    <strong>"말하면, 글이 돼요."</strong><br />
    단축키 두 번으로 현재 커서 위치에 바로 입력하는 macOS용 100% 온디바이스 음성 받아쓰기 앱
  </p>

  <p>
    <img src="https://img.shields.io/badge/platform-macOS-000000?style=flat-square&logo=apple&logoColor=white" alt="macOS" />
    <img src="https://img.shields.io/badge/Tauri-v2-24C8D8?style=flat-square&logo=tauri&logoColor=white" alt="Tauri v2" />
    <img src="https://img.shields.io/badge/Rust-2021-DEA584?style=flat-square&logo=rust&logoColor=white" alt="Rust" />
    <img src="https://img.shields.io/badge/React-19-61DAFB?style=flat-square&logo=react&logoColor=black" alt="React 19" />
    <img src="https://img.shields.io/badge/Whisper-large--v3--turbo-412991?style=flat-square" alt="Whisper large-v3-turbo" />
    <img src="https://img.shields.io/badge/license-MIT-3B5BDB?style=flat-square" alt="MIT License" />
  </p>
</div>

---

## 개요

**또박(Ttobak)**은 OpenAI `whisper-large-v3-turbo`와 Apple Metal GPU 가속, Silero VAD v6 음성 구간 검출을 결합해 **음성과 텍스트가 Mac 밖으로 단 1바이트도 나가지 않는** 네이티브 메뉴 막대 받아쓰기 앱입니다.

외부 클라우드 API나 계정 로그인 없이 로컬 환경만으로 동작하며, 한국어와 영어가 섞인 발화도 자연스럽게 인식해 현재 작업 중인 입력창에 즉시 입력합니다.

---

## 사용 방법

> **누르고 · 말하고 · 다시 누르면 끝.**

| 단계 | 동작 | 설명 |
| :--- | :--- | :--- |
| **1. 누르고** | 단축키 입력 (기본 `우측 ⌥`) | 커서가 놓인 곳에서 녹음을 시작하고, 화면 상단에 플로팅 상태 캡슐을 띄웁니다. |
| **2. 말하고** | 자연스럽게 발화 | 최대 10분까지 연속 발화가 가능하며, 한·영 혼용 발화를 자동으로 처리합니다. |
| **3. 다시 누르면** | 단축키 재입력 | 변환된 텍스트를 현재 커서 위치에 바로 붙여넣습니다. (`Esc`로 즉시 취소) |

---

## 주요 기능

- **100% 온디바이스 음성 인식 & Metal GPU 가속**
  - 모든 음성 분석과 텍스트 변환이 사용자의 Mac 내부에서 완결됩니다.
  - Apple Silicon 통합 메모리 위에서 Metal + FlashAttention 가속을 통해 빠르게 추론합니다.
- **모델 선택 지원 (FP16 / Q8_0)**
  - **FP16 (`~1.62 GB`)**: 무양자화 원본 가중치 기반의 최고 정확도 기본 모델
  - **Q8_0 (`~874 MB`)**: 메모리 사용량을 절반 가까이 줄인 경량 고속 모델
- **Silero VAD v6 무음 제거**
  - 추론 전 발화 구간만 정밀하게 잘라내어 무음·배경 소음으로 인한 환각(Hallucination)을 방지합니다.
- **용어 사전 (최대 80 BPE 토큰)**
  - 고유명사, 프로젝트명, 사내 약어, 영문 기술 용어를 칩(Chip) UI로 간편하게 등록·관리합니다.
  - 실제 Whisper BPE 토크나이저로 사용량을 실시간 계산하며, 문장부호 컨디셔닝 프롬프트에 반영합니다.
- **클립보드 보존 입력 & 복구 카드**
  - 기존 클립보드 내용을 백업한 뒤 `TransientType` 마커와 함께 `Cmd+V`로 삽입하고 자동으로 원상 복원합니다.
  - 입력 가능한 포커스가 없는 경우 **복구 카드**를 띄워 한 번의 클릭으로 복사할 수 있습니다.
- **2중 보안 입력 차단 & 임시 파일 즉시 파기**
  - `IsSecureEventInputEnabled()`와 `AXSecureTextField`를 녹음 시작 전·입력 직전 2중 검사해 비밀번호 입력란에서는 동작하지 않습니다.
  - 임시 녹음 파일은 변환 직후 즉시 삭제됩니다.

---

## 기술 스택 및 아키텍처

```text
마이크 캡처 (cpal · 16kHz 리샘플링)
   └─▶ 무음 구간 제거 (ort · Silero VAD v6)
          └─▶ 온디바이스 STT (whisper-rs · Metal GPU · 용어 사전 프롬프트)
                 └─▶ 커서 위치 자동 입력 (클립보드 스냅샷 · Cmd+V · 복구 카드)
```

| 영역 | 기술 및 크레이트 | 역할 |
| :--- | :--- | :--- |
| **앱 프레임워크** | Tauri v2 (`LSUIElement`) | macOS 메뉴 막대 트레이 및 멀티 윈도우(`menu`, `hud`, `settings`) 관리 |
| **프론트엔드** | React 19, TypeScript, Vite, Lucide | 메뉴 팝오버, 플로팅 캡슐 HUD, 설정 창 UI |
| **음성 인식** | `whisper-rs`, `ort`, `tokenizers` | Whisper `large-v3-turbo` 추론, Silero VAD v6, BPE 토큰 예산 계산 |
| **오디오 파이프라인** | `cpal`, `rubato`, `hound` | 저지연 오디오 캡처, 16kHz 모노 리샘플링, RMS 파형 레벨 계산 |
| **macOS 통합** | `CGEventTap`, Accessibility API, `SMAppService` | 전역 단축키 감지, 보안 입력란 검사, 클립보드 보존 붙여넣기, 로그인 시 자동 시작 |

---

## 시작하기

### 사전 요구사항

- **OS**: macOS (Apple Silicon 권장)
- **도구체인**: Rust (`stable`), Node.js (v20+), `pnpm`

### 개발 및 빌드

```bash
# 1. 프론트엔드 의존성 설치
pnpm install

# 2. 전체 Rust 워크스페이스 테스트 실행
cargo test --workspace

# 3. 개발 모드로 실행
pnpm tauri dev

# 4. macOS 릴리스 앱 번들(.app) 빌드
pnpm tauri build --bundles app
```

### 권한 안내

처음 실행 후 아래 두 가지 macOS 시스템 권한을 허용해야 정상적으로 받아쓰기를 사용할 수 있습니다.

1. **마이크**: 음성 녹음을 위해 필요
2. **손쉬운 사용 (접근성)**: 현재 활성화된 입력란 확인 및 `Cmd+V` 자동 입력을 위해 필요

---

## 프로젝트 구조

```text
.
├── crates/
│   ├── ttobak-core/       # 설정, 상태 머신 타입, 로컬 통계, 키워드 파서
│   ├── ttobak-audio/      # 오디오 캡처, 리샘플링, RMS 미터, 임시 WAV 관리
│   ├── ttobak-stt/        # Silero VAD v6, Whisper Metal 엔진, 프롬프트 빌더, 모델 다운로더
│   └── ttobak-macos/      # 전역 단축키, 포커스·보안 검사, 클립보드 주입, 권한 관리
├── src-tauri/             # Tauri v2 백엔드, 트레이 메뉴, 세션 오케스트레이터
├── src/                   # React 19 UI (메뉴 팝오버, 플로팅 HUD, 설정 창)
├── Cargo.toml             # Rust 워크스페이스 매니페스트
└── package.json           # 프론트엔드 스크립트 및 의존성
```

---

## 라이선스

이 프로젝트는 [MIT License](Cargo.toml)에 따라 배포됩니다.
