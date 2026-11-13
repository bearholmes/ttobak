# 또박 (Ttobak)

> **"말하면, 글이 돼요."** — macOS용 100% 온디바이스 한국어 음성 받아쓰기 메뉴 막대 앱

**또박(Ttobak)**은 OpenAI `whisper-large-v3-turbo` 무양자화 FP16 모델(`ggml-large-v3-turbo.bin`)과 Apple Metal GPU 가속, Silero VAD v6 무음 제거기를 결합하여 **음성과 텍스트가 Mac 밖으로 단 1바이트도 나가지 않는** Rust + Tauri v2 + React 기반 온디바이스 받아쓰기 애플리케이션입니다.

## 주요 특징

- **단축키 두 번 (`PRESS · SPEAK · PRESS`)**: 기본 `우측 ⌥` 키를 한 번 눌러 녹음하고(`듣고 있어요` 플로팅 캡슐), 다시 눌러 현재 커서 위치에 바로 텍스트를 입력합니다. 취소는 `Esc` 한 번으로 가능합니다.
- **무양자화 FP16 기본 + Q8_0 경량 모델 선택 지원 (Mac GPU 가속)**: 기본값은 OpenAI `2024-09-30` `whisper-large-v3-turbo` 원본 가중치와 동일한 `ggml-large-v3-turbo.bin` (~1.62GB)이며, 통합 메모리(RAM)를 절약하고 싶을 때는 설정에서 `ggml-large-v3-turbo-q8_0.bin` (~874MB) 경량 모델로 언제든 전환할 수 있습니다.
- **Silero VAD v6 사전 무음 제거**: 추론 전 `silero_vad_v6.onnx`로 무음 구간을 잘라내어 백그라운드 환각(Hallucination)을 원천 방지합니다.
- **용어 사전 (`80` BPE 토큰 예산)**: 실제 Whisper BPE 토크나이저로 80 토큰 예산을 실시간 계산하고 2-Tier 한국어 문장부호 컨디셔닝 프롬프트 및 에코 필터를 적용합니다.
- **클립보드 보존 주입 및 복구 카드**: 기존 클립보드 멀티 아이템/UTI 스냅샷을 백업한 뒤 `TransientType` 마커와 함께 `Cmd+V`로 입력하고 원상 복원합니다. 입력창 포커스가 없으면 **복구 카드**를 띄워 한 번에 복사할 수 있게 합니다.
- **비밀번호 입력란 2중 차단 & 임시 파일 즉시 삭제**: Carbon `IsSecureEventInputEnabled()`와 Accessibility `AXSecureTextField`를 녹음 전·붙여넣기 직전 2중 검사하며, 임시 녹음 파일은 변환 직후 즉시 삭제됩니다.

## 워크스페이스 구조

- `crates/ttobak-core`: 도메인 설정, 세션 상태 머신 타입, 로컬 전용 통계, 키워드 파서
- `crates/ttobak-audio`: `cpal` 실시간 캡처, 16kHz 모노 리샘플링, RMS 파형 레벨, 10분 자동 종료 및 RAII 임시 WAV 정리
- `crates/ttobak-stt`: `ort` Silero VAD v6 트리머, `whisper-rs` Metal 추론 엔진, 80 BPE 토큰 프롬프트 빌더, 이어받기 모델 다운로더
- `crates/ttobak-macos`: 전역 단축키(`CGEventTap`), 보안 입력·포커스 검사, 클립보드 보존 `Cmd+V` 합성, 권한 및 `LaunchAgent` 관리
- `src-tauri`: Tauri v2 메뉴 막대(`LSUIElement`) 트레이, 멀티 윈도우(`menu`, `hud`, `settings`) 오케스트레이터
- `src`: React 19 + TypeScript 멀티 윈도우 UI

## 개발 및 빌드 방법

```bash
# 프론트엔드 패키지 설치 및 번들 빌드
pnpm install
pnpm build

# 전체 Rust 크레이트 단위·통합 테스트 실행
cargo test --workspace

# 개발 모드 실행
pnpm tauri dev

# macOS 릴리스 앱 번들(또박.app) 생성
pnpm tauri build --bundles app
```

빌드된 앱 번들은 `target/release/bundle/macos/또박.app`에 생성됩니다.
