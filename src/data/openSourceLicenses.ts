export interface OpenSourceItem {
  name: string;
  version: string;
  license: string;
  spdxUrl: string;
  copyright: string;
  role: string;
  repoUrl: string;
}

export interface OpenSourceCategory {
  category: string;
  items: OpenSourceItem[];
}

export const OPEN_SOURCE_CATEGORIES: OpenSourceCategory[] = [
  {
    category: "AI 모델 및 음성 추론 엔진",
    items: [
      {
        name: "OpenAI Whisper (large-v3-turbo)",
        version: "large-v3-turbo",
        license: "MIT",
        spdxUrl: "https://spdx.org/licenses/MIT.html",
        copyright: "OpenAI",
        role: "온디바이스 한국어·다국어 음성 인식 모델 가중치 및 BPE 토크나이저",
        repoUrl: "https://github.com/openai/whisper",
      },
      {
        name: "whisper.cpp & ggml",
        version: "1.7+",
        license: "MIT",
        spdxUrl: "https://spdx.org/licenses/MIT.html",
        copyright: "Georgi Gerganov & ggml authors",
        role: "Apple Silicon Metal GPU 가속 C/C++ 음성 인식 추론 엔진",
        repoUrl: "https://github.com/ggml-org/whisper.cpp",
      },
      {
        name: "whisper-rs",
        version: "0.16.0",
        license: "MIT / Unlicense",
        spdxUrl: "https://spdx.org/licenses/MIT.html",
        copyright: "Tazz4843 & contributors",
        role: "whisper.cpp 네이티브 추론 엔진의 Rust 바인딩",
        repoUrl: "https://github.com/tazz4843/whisper-rs",
      },
      {
        name: "Silero VAD",
        version: "v6 (ONNX)",
        license: "MIT",
        spdxUrl: "https://spdx.org/licenses/MIT.html",
        copyright: "Silero Team",
        role: "실시간 음성 활동 감지(VAD) 및 무음 구간 트리밍 신경망 모델",
        repoUrl: "https://github.com/snakers4/silero-vad",
      },
      {
        name: "ONNX Runtime & ort",
        version: "2.0",
        license: "MIT / Apache-2.0",
        spdxUrl: "https://spdx.org/licenses/Apache-2.0.html",
        copyright: "Microsoft Corporation / pyke.io",
        role: "Silero VAD ONNX 모델 실행 런타임 및 Rust 인터페이스",
        repoUrl: "https://github.com/pykeio/ort",
      },
      {
        name: "Hugging Face Tokenizers",
        version: "0.21.4",
        license: "Apache-2.0",
        spdxUrl: "https://spdx.org/licenses/Apache-2.0.html",
        copyright: "Hugging Face Inc.",
        role: "자주 쓰는 말 프롬프트의 Whisper BPE 30 토큰 예산 계산",
        repoUrl: "https://github.com/huggingface/tokenizers",
      },
    ],
  },
  {
    category: "데스크톱 프레임워크 및 오디오 처리",
    items: [
      {
        name: "Tauri",
        version: "2.x",
        license: "MIT / Apache-2.0",
        spdxUrl: "https://spdx.org/licenses/MIT.html",
        copyright: "Tauri Programme within The Commons Conservancy",
        role: "macOS 메뉴 막대 앱 프레임워크 및 멀티 윈도우 런타임",
        repoUrl: "https://github.com/tauri-apps/tauri",
      },
      {
        name: "cpal",
        version: "0.15",
        license: "Apache-2.0",
        spdxUrl: "https://spdx.org/licenses/Apache-2.0.html",
        copyright: "RustAudio Developers",
        role: "CoreAudio 기반 실시간 마이크 오디오 캡처",
        repoUrl: "https://github.com/RustAudio/cpal",
      },
      {
        name: "hound",
        version: "3.5",
        license: "Apache-2.0",
        spdxUrl: "https://spdx.org/licenses/Apache-2.0.html",
        copyright: "Ruud van Asseldonk",
        role: "16kHz Mono PCM WAV 인코딩 및 입출력",
        repoUrl: "https://github.com/ruuda/hound",
      },
    ],
  },
  {
    category: "macOS 네이티브 연동 및 Rust 코어",
    items: [
      {
        name: "objc2 / objc2-app-kit / objc2-foundation / block2",
        version: "0.6 / 0.3",
        license: "MIT",
        spdxUrl: "https://spdx.org/licenses/MIT.html",
        copyright: "Mads Marquart & contributors",
        role: "macOS AppKit·NSWorkspace·NSPasteboard·NSEvent 네이티브 연동",
        repoUrl: "https://github.com/madsmtm/objc2",
      },
      {
        name: "core-foundation-rs",
        version: "0.10.1",
        license: "MIT / Apache-2.0",
        spdxUrl: "https://spdx.org/licenses/MIT.html",
        copyright: "The Servo Project Developers",
        role: "macOS CoreFoundation 및 손쉬운 사용(AXUIElement) 제어",
        repoUrl: "https://github.com/servo/core-foundation-rs",
      },
      {
        name: "Tokio",
        version: "1.42",
        license: "MIT",
        spdxUrl: "https://spdx.org/licenses/MIT.html",
        copyright: "Tokio Contributors",
        role: "비동기 이벤트 루프 및 백그라운드 작업 스케줄링",
        repoUrl: "https://github.com/tokio-rs/tokio",
      },
      {
        name: "Serde & serde_json",
        version: "1.0",
        license: "MIT / Apache-2.0",
        spdxUrl: "https://spdx.org/licenses/MIT.html",
        copyright: "Erick Tryzelaar, David Tolnay",
        role: "설정 파일 저장 및 상태 스냅샷 직렬화",
        repoUrl: "https://github.com/serde-rs/serde",
      },
      {
        name: "Reqwest & Rustls",
        version: "0.12",
        license: "MIT / Apache-2.0",
        spdxUrl: "https://spdx.org/licenses/MIT.html",
        copyright: "Sean McArthur & Rustls Contributors",
        role: "온디바이스 모델 이어받기 다운로드 및 업데이트 확인용 HTTPS 통신",
        repoUrl: "https://github.com/seanmonstar/reqwest",
      },
      {
        name: "Rust 유틸리티 크레이트 모음",
        version: "anyhow · thiserror · tracing · parking_lot · sha2 · hex · uuid · dirs",
        license: "MIT / Apache-2.0",
        spdxUrl: "https://spdx.org/licenses/MIT.html",
        copyright: "Rust Community Contributors",
        role: "오류 처리, 로깅, 동기화, SHA-256 검증 및 사용자 경로 탐색",
        repoUrl: "https://crates.io",
      },
    ],
  },
  {
    category: "프론트엔드 UI 라이브러리",
    items: [
      {
        name: "React & React DOM",
        version: "19.x",
        license: "MIT",
        spdxUrl: "https://spdx.org/licenses/MIT.html",
        copyright: "Meta Platforms, Inc. and affiliates",
        role: "설정 화면, 메뉴 막대 팝오버 및 플로팅 HUD 인터페이스 렌더링",
        repoUrl: "https://github.com/facebook/react",
      },
      {
        name: "Lucide Icons (lucide-react)",
        version: "0.469",
        license: "ISC",
        spdxUrl: "https://spdx.org/licenses/ISC.html",
        copyright: "Lucide Contributors",
        role: "앱 전반의 벡터 아이콘 구성 요소",
        repoUrl: "https://github.com/lucide-icons/lucide",
      },
    ],
  },
];
