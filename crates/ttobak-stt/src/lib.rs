pub mod downloader;
pub mod prompt;
pub mod vad;
pub mod whisper;

use anyhow::Result;
use downloader::ModelFiles;
use prompt::PromptTokenizer;
use tracing::info;
use vad::SpeechActivityTrimmer;
use whisper::WhisperEngine;

/// Silero VAD v6 + Whisper large-v3-turbo (FP16 Metal) 통합 온디바이스 파이프라인
pub struct SttPipeline {
    pub vad: SpeechActivityTrimmer,
    pub whisper: WhisperEngine,
    pub tokenizer: PromptTokenizer,
}

#[derive(Debug, Clone)]
pub struct TranscriptionOutcome {
    pub transcript: String,
    pub speech_segments: usize,
    pub trimmed_duration_secs: f64,
}

impl SttPipeline {
    pub fn load(files: &ModelFiles) -> Result<Self> {
        let vad = SpeechActivityTrimmer::load(&files.silero_onnx)?;
        let whisper = WhisperEngine::load(&files.whisper_bin)?;
        let tokenizer = PromptTokenizer::from_file_optional(&files.tokenizer_json);
        Ok(Self {
            vad,
            whisper,
            tokenizer,
        })
    }

    /// 16kHz 모노 PCM 입력에 대해 무음 트리밍 → 2-Tier 프롬프트 생성 → Metal GPU Whisper 디코딩을 수행합니다.
    pub fn transcribe_samples(
        &self,
        samples_16k: &[f32],
        raw_keywords: &str,
    ) -> Result<TranscriptionOutcome> {
        let vad_res = self.vad.trim(samples_16k)?;
        if vad_res.trimmed_samples.is_empty() {
            info!("Silero VAD 판정: 발화 구간 없음 (무음 입력 조기 종료)");
            return Ok(TranscriptionOutcome {
                transcript: String::new(),
                speech_segments: 0,
                trimmed_duration_secs: 0.0,
            });
        }

        let trimmed_duration_secs = vad_res.trimmed_samples.len() as f64 / 16_000.0;
        let prompts = self.tokenizer.build_prompts(raw_keywords);
        let transcript = self
            .whisper
            .transcribe(&vad_res.trimmed_samples, &prompts)?;

        Ok(TranscriptionOutcome {
            transcript,
            speech_segments: vad_res.segments.len(),
            trimmed_duration_secs,
        })
    }
}
