use std::fs;
use ttobak_stt::downloader::{
    DownloadProgress, SILERO_VAD_SHA256, SILERO_VAD_URL, TOKENIZER_URL,
};
use ttobak_stt::prompt::PromptTokenizer;
use ttobak_stt::vad::SpeechActivityTrimmer;

#[tokio::test]
async fn test_real_silero_vad_onnx_and_whisper_tokenizer() {
    let tmp_dir = std::env::temp_dir().join("ttobak_integration_smoke");
    fs::create_dir_all(&tmp_dir).unwrap();

    let vad_path = tmp_dir.join("silero_vad_v6.onnx");
    if !vad_path.exists() {
        let bytes = reqwest::get(SILERO_VAD_URL)
            .await
            .expect("Silero VAD 다운로드 실패")
            .bytes()
            .await
            .unwrap();
        use sha2::{Digest, Sha256};
        let actual_sha = hex::encode(Sha256::digest(&bytes));
        assert_eq!(actual_sha, SILERO_VAD_SHA256);
        fs::write(&vad_path, &bytes).unwrap();
    }

    // 실제 ONNX Runtime으로 Silero VAD v6 로드 및 무음 2초(32,000 샘플) 추론 검증
    let trimmer = SpeechActivityTrimmer::load(&vad_path).expect("Silero VAD ONNX 세션 생성 실패");
    let silence_2s = vec![0.0f32; 32_000];
    let out = trimmer.trim(&silence_2s).expect("Silero VAD 추론 실패");
    assert!(
        out.trimmed_samples.is_empty(),
        "완전 무음 입력은 빈 샘플로 트리밍되어야 합니다"
    );
    assert_eq!(out.segments.len(), 0);

    // 실제 Whisper large-v3-turbo tokenizer.json 로드 및 30 BPE 토큰 예산 검증
    let tok_path = tmp_dir.join("tokenizer.json");
    if !tok_path.exists() {
        let bytes = reqwest::get(TOKENIZER_URL)
            .await
            .expect("Whisper tokenizer.json 다운로드 실패")
            .bytes()
            .await
            .unwrap();
        fs::write(&tok_path, &bytes).unwrap();
    }

    let tokenizer = PromptTokenizer::from_file_optional(&tok_path);
    assert!(tokenizer.is_exact(), "실제 Hugging Face BPE 토크나이저가 활성화되어야 합니다");

    let usage = tokenizer.analyze_keywords("또박, 품의서, 클로드 코드, Tauri");
    assert!(!usage.is_over_limit);
    assert_eq!(usage.usable_keyword_count, 4);
    assert!(usage.tokens > 0 && usage.tokens <= 30);

    let _ = DownloadProgress {
        stage: "ok".to_string(),
        downloaded_bytes: 1,
        total_bytes: 1,
        fraction: 1.0,
    };
}
