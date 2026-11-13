use crate::prompt::{strip_hint_fragments, ConditionedPrompts};
use anyhow::{anyhow, Context, Result};
use parking_lot::Mutex;
use std::path::Path;
use std::sync::Once;
use tracing::{debug, info};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

/// 초단문 오디오 끝 음절 잘림 방지를 위한 최소 길이 (`(1.0 + 0.1) * 16_000 = 17,600` 샘플 = 1.1초)
pub const MIN_PADDED_SAMPLES_16K: usize = 17_600;
/// Whisper 세그먼트 무음 확률 임계치
pub const NO_SPEECH_THRESHOLD: f32 = 0.60;

static LOG_HOOK_INIT: Once = Once::new();

/// 초단문 오디오(< 1.1초) 뒤에 무음 패딩을 덧붙여 마지막 음절 유실을 막습니다.
pub fn pad_past_end_clip(samples_16k: &[f32]) -> Vec<f32> {
    if samples_16k.is_empty() {
        return Vec::new();
    }
    // 일반 발화에도 끝에 120ms(1,920 샘플) 여유 무음을 추가하고 최소 17,600 샘플을 보장
    let target_len = (samples_16k.len() + 1_920).max(MIN_PADDED_SAMPLES_16K);
    let mut out = Vec::with_capacity(target_len);
    out.extend_from_slice(samples_16k);
    out.resize(target_len, 0.0);
    out
}

/// Apple Metal GPU 가속 기반 `ggml-large-v3-turbo.bin` (FP16) 추론 엔진
pub struct WhisperEngine {
    ctx: Mutex<WhisperContext>,
    n_threads: i32,
}

impl WhisperEngine {
    pub fn load(model_path: &Path) -> Result<Self> {
        LOG_HOOK_INIT.call_once(|| {
            whisper_rs::install_logging_hooks();
        });

        let model_path_str = model_path
            .to_str()
            .ok_or_else(|| anyhow!("모델 경로가 유효한 UTF-8이 아닙니다: {:?}", model_path))?;

        let mut ctx_params = WhisperContextParameters::default();
        ctx_params.use_gpu(true);
        ctx_params.flash_attn(true);

        let ctx = WhisperContext::new_with_params(model_path_str, ctx_params)
            .with_context(|| format!("Whisper 모델 초기화 실패: {:?}", model_path))?;

        let perf_cores = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4)
            .clamp(2, 8) as i32;

        info!(
            "Whisper large-v3-turbo (Metal GPU + FlashAttn, CPU threads={}) 로드 완료",
            perf_cores
        );

        Ok(Self {
            ctx: Mutex::new(ctx),
            n_threads: perf_cores,
        })
    }

    /// VAD 트리밍이 완료된 16kHz 샘플을 받아 2-Tier 프롬프트 폴백 방식으로 한국어 텍스트를 추론합니다.
    pub fn transcribe(&self, trimmed_samples_16k: &[f32], prompts: &ConditionedPrompts) -> Result<String> {
        if trimmed_samples_16k.is_empty() {
            return Ok(String::new());
        }

        let padded = pad_past_end_clip(trimmed_samples_16k);
        let ctx = self.ctx.lock();

        // Tier 1: 자주 쓰는 말이 있으면 키워드 포함 프롬프트로 먼저 추론
        if let Some(tier1_prompt) = &prompts.tier1_with_keywords {
            if let Ok(text) = self.decode_once(&ctx, &padded, tier1_prompt) {
                let cleaned = strip_hint_fragments(&text);
                if !cleaned.is_empty() {
                    debug!("Tier 1 (키워드 프롬프트) 추론 성공: {}자", cleaned.chars().count());
                    return Ok(cleaned);
                }
            }
        }

        // Tier 2: 기본 한국어 문장부호 유도 프롬프트로 추론 (또는 Tier 1 실패 시 폴백)
        let text = self.decode_once(&ctx, &padded, &prompts.tier2_base)?;
        Ok(strip_hint_fragments(&text))
    }

    fn decode_once(&self, ctx: &WhisperContext, pcm_16k: &[f32], prompt_text: &str) -> Result<String> {
        let mut state = ctx
            .create_state()
            .map_err(|e| anyhow!("Whisper 추론 상태 생성 실패: {}", e))?;

        // `set_initial_prompt`의 C 문자열 누수를 피하기 위해 GGML 내장 토크나이저로 직접 토큰화하여 전달
        let prompt_tokens = ctx.tokenize(prompt_text, 512).unwrap_or_default();

        // 정적 언어 코드 C 문자열 ("ko\0")을 정적 바이트에서 직접 참조
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 5 });
        params.set_n_threads(self.n_threads);
        params.set_translate(false);
        params.set_no_context(false);
        params.set_no_timestamps(true);
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        params.set_suppress_blank(true);
        params.set_suppress_nst(true);
        params.set_temperature(0.0);
        params.set_temperature_inc(0.2);
        params.set_entropy_thold(2.4);
        params.set_logprob_thold(-1.0);
        params.set_no_speech_thold(NO_SPEECH_THRESHOLD);
        params.set_language(Some("ko"));

        if !prompt_tokens.is_empty() {
            params.set_tokens(&prompt_tokens);
        }

        state
            .full(params, pcm_16k)
            .map_err(|e| anyhow!("Whisper 디코딩 실행 실패: {}", e))?;

        let mut segments_out = Vec::new();
        for seg in state.as_iter() {
            if seg.no_speech_probability() >= NO_SPEECH_THRESHOLD {
                continue;
            }
            if let Ok(piece) = seg.to_str_lossy() {
                let trimmed = piece.trim();
                if !trimmed.is_empty() {
                    segments_out.push(trimmed.to_string());
                }
            }
        }

        Ok(segments_out.join(" "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pad_past_end_clip_enforces_1_1_sec_minimum() {
        let short = vec![0.1f32; 4_000]; // 0.25초
        let padded = pad_past_end_clip(&short);
        assert_eq!(padded.len(), MIN_PADDED_SAMPLES_16K);
        assert!((padded[0] - 0.1).abs() < 1e-6);
        assert!((padded[17_599]).abs() < 1e-6);
    }

    #[test]
    fn test_pad_past_end_clip_adds_tail_margin_for_long_audio() {
        let normal = vec![0.05f32; 32_000]; // 2.0초
        let padded = pad_past_end_clip(&normal);
        assert_eq!(padded.len(), 33_920);
    }
}
