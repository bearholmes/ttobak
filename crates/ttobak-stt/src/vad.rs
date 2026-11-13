use anyhow::{anyhow, Context, Result};
use ort::{
    environment::Environment,
    session::{builder::GraphOptimizationLevel, Session},
    value::Tensor,
};
use parking_lot::Mutex;
use std::path::Path;
use std::sync::OnceLock;

/// 16kHz 기준 Silero VAD v6 윈도우 크기 (32ms = 512 샘플)
pub const VAD_WINDOW_SAMPLES: usize = 512;
/// 16kHz 기준 Silero VAD v6 좌측 컨텍스트 크기 (4ms = 64 샘플)
pub const VAD_CONTEXT_SAMPLES: usize = 64;
/// ONNX 입력 행 너비 (64 + 512 = 576)
pub const VAD_FRAME_WIDTH: usize = VAD_CONTEXT_SAMPLES + VAD_WINDOW_SAMPLES;
/// 배치 추론 최대 윈도우 수 (10,000 윈도우 ≈ 320초 단위 배치)
pub const VAD_BATCH_WINDOWS: usize = 10_000;

/// 음성 시작 임계값 (`SpeechActivityTrimmer`)
pub const SPEECH_THRESHOLD: f32 = 0.50;
/// 음성 종료 임계값 (히스테리시스 `threshold - 0.15`)
pub const NEG_SPEECH_THRESHOLD: f32 = 0.35;

/// 최소 발화 길이 (250ms = 4,000 샘플)
pub const MIN_SPEECH_SAMPLES: usize = 4_000;
/// 발화 간 병합 기준 무음 길이 (400ms = 6,400 샘플)
pub const MIN_SILENCE_SAMPLES: usize = 6_400;
/// 발화 앞뒤 여유 패딩 (160ms = 2,560 샘플)
pub const SPEECH_PAD_SAMPLES: usize = 2_560;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpeechSegment {
    pub start_sample: usize,
    pub end_sample: usize,
}

#[derive(Debug, Clone)]
pub struct VadTrimResult {
    pub trimmed_samples: Vec<f32>,
    pub segments: Vec<SpeechSegment>,
}

fn global_ort_env() -> Result<&'static Environment> {
    static ENV: OnceLock<Environment> = OnceLock::new();
    if let Some(env) = ENV.get() {
        return Ok(env);
    }
    let built = ort::init()
        .with_name("ttobak-silero-vad")
        .build()
        .map_err(|e| anyhow!("ONNX Runtime 환경 초기화 실패: {}", e))?;
    let _ = ENV.set(built);
    ENV.get()
        .ok_or_else(|| anyhow!("ONNX Runtime 환경을 가져올 수 없습니다."))
}

/// 윈도우별 발화 확률 배열을 히스테리시스 기반으로 병합해 발화 구간(샘플 인덱스) 목록으로 변환합니다.
pub fn extract_speech_segments(probs: &[f32], total_samples: usize) -> Vec<SpeechSegment> {
    if probs.is_empty() || total_samples == 0 {
        return Vec::new();
    }

    let mut raw_segments: Vec<SpeechSegment> = Vec::new();
    let mut triggered = false;
    let mut current_start: usize = 0;
    let mut temp_end: Option<usize> = None;

    for (idx, &prob) in probs.iter().enumerate() {
        let win_sample = idx * VAD_WINDOW_SAMPLES;

        if prob >= SPEECH_THRESHOLD && temp_end.is_some() {
            temp_end = None;
        }

        if prob >= SPEECH_THRESHOLD && !triggered {
            triggered = true;
            current_start = win_sample;
            continue;
        }

        if prob < NEG_SPEECH_THRESHOLD && triggered {
            let end_anchor = *temp_end.get_or_insert(win_sample);
            if win_sample.saturating_sub(end_anchor) >= MIN_SILENCE_SAMPLES {
                if end_anchor.saturating_sub(current_start) >= MIN_SPEECH_SAMPLES {
                    raw_segments.push(SpeechSegment {
                        start_sample: current_start,
                        end_sample: end_anchor.min(total_samples),
                    });
                }
                triggered = false;
                temp_end = None;
            }
        }
    }

    if triggered {
        let final_end = total_samples;
        if final_end.saturating_sub(current_start) >= MIN_SPEECH_SAMPLES / 2 {
            raw_segments.push(SpeechSegment {
                start_sample: current_start,
                end_sample: final_end,
            });
        }
    }

    if raw_segments.is_empty() {
        return Vec::new();
    }

    // 앞뒤 여유 패딩 부여 후 인접/중복 세그먼트 병합
    let mut merged: Vec<SpeechSegment> = Vec::with_capacity(raw_segments.len());
    for seg in raw_segments {
        let padded = SpeechSegment {
            start_sample: seg.start_sample.saturating_sub(SPEECH_PAD_SAMPLES),
            end_sample: (seg.end_sample + SPEECH_PAD_SAMPLES).min(total_samples),
        };
        if let Some(last) = merged.last_mut() {
            // 세그먼트 사이 간격이 200ms(3,200 샘플) 이하이면 하나로 합침
            if padded.start_sample <= last.end_sample + 3_200 {
                last.end_sample = last.end_sample.max(padded.end_sample);
                continue;
            }
        }
        merged.push(padded);
    }

    merged
}

/// Silero VAD v6 (`silero_vad_v6.onnx`) 기반 무음 구간 제거기
pub struct SpeechActivityTrimmer {
    session: Mutex<Session>,
}

impl SpeechActivityTrimmer {
    pub fn load(onnx_path: &Path) -> Result<Self> {
        let env = global_ort_env()?;
        let session = Session::builder(env)
            .map_err(|e| anyhow!("VAD 세션 빌더 생성 실패: {}", e))?
            .with_optimization_level(GraphOptimizationLevel::Level3)
            .map_err(|e| anyhow!("VAD 최적화 레벨 설정 실패: {}", e))?
            .with_intra_threads(2)
            .map_err(|e| anyhow!("VAD 스레드 설정 실패: {}", e))?
            .commit_from_file(onnx_path)
            .with_context(|| format!("Silero VAD 모델 로드 실패: {:?}", onnx_path))?;

        Ok(Self {
            session: Mutex::new(session),
        })
    }

    /// 16kHz 모노 PCM에서 무음 구간을 잘라내고 실제 발화 샘플만 연결해 반환합니다.
    pub fn trim(&self, samples_16k: &[f32]) -> Result<VadTrimResult> {
        if samples_16k.is_empty() {
            return Ok(VadTrimResult {
                trimmed_samples: Vec::new(),
                segments: Vec::new(),
            });
        }

        let num_windows = samples_16k.len().div_ceil(VAD_WINDOW_SAMPLES);
        let padded_len = num_windows * VAD_WINDOW_SAMPLES;
        let mut padded = Vec::with_capacity(padded_len);
        padded.extend_from_slice(samples_16k);
        padded.resize(padded_len, 0.0);

        let mut all_probs = Vec::with_capacity(num_windows);
        let mut h_state = vec![0.0f32; 128];
        let mut c_state = vec![0.0f32; 128];
        let mut prev_context = [0.0f32; VAD_CONTEXT_SAMPLES];

        let mut session = self.session.lock();

        for batch_start in (0..num_windows).step_by(VAD_BATCH_WINDOWS) {
            let batch_count = (num_windows - batch_start).min(VAD_BATCH_WINDOWS);
            let mut batch_matrix = Vec::with_capacity(batch_count * VAD_FRAME_WIDTH);

            for w in 0..batch_count {
                let global_w = batch_start + w;
                let win_offset = global_w * VAD_WINDOW_SAMPLES;
                let window_slice = &padded[win_offset..win_offset + VAD_WINDOW_SAMPLES];

                batch_matrix.extend_from_slice(&prev_context);
                batch_matrix.extend_from_slice(window_slice);

                prev_context.copy_from_slice(
                    &window_slice[VAD_WINDOW_SAMPLES - VAD_CONTEXT_SAMPLES..VAD_WINDOW_SAMPLES],
                );
            }

            let input_tensor = Tensor::from_array(([batch_count, VAD_FRAME_WIDTH], batch_matrix))
                .map_err(|e| anyhow!("VAD input 텐서 생성 실패: {}", e))?;
            let h_tensor = Tensor::from_array(([1usize, 1usize, 128usize], h_state.clone()))
                .map_err(|e| anyhow!("VAD h 텐서 생성 실패: {}", e))?;
            let c_tensor = Tensor::from_array(([1usize, 1usize, 128usize], c_state.clone()))
                .map_err(|e| anyhow!("VAD c 텐서 생성 실패: {}", e))?;

            let outputs = session
                .run(ort::inputs![
                    "input" => input_tensor,
                    "h" => h_tensor,
                    "c" => c_tensor,
                ])
                .map_err(|e| anyhow!("Silero VAD 추론 실패: {}", e))?;

            let (_, probs_slice) = outputs["speech_probs"]
                .try_extract_tensor::<f32>()
                .map_err(|e| anyhow!("VAD speech_probs 추출 실패: {}", e))?;
            all_probs.extend_from_slice(probs_slice);

            let (_, hn_slice) = outputs["hn"]
                .try_extract_tensor::<f32>()
                .map_err(|e| anyhow!("VAD hn 추출 실패: {}", e))?;
            if hn_slice.len() == 128 {
                h_state.copy_from_slice(hn_slice);
            }

            let (_, cn_slice) = outputs["cn"]
                .try_extract_tensor::<f32>()
                .map_err(|e| anyhow!("VAD cn 추출 실패: {}", e))?;
            if cn_slice.len() == 128 {
                c_state.copy_from_slice(cn_slice);
            }
        }

        let segments = extract_speech_segments(&all_probs, samples_16k.len());
        if segments.is_empty() {
            return Ok(VadTrimResult {
                trimmed_samples: Vec::new(),
                segments,
            });
        }

        let total_trimmed_len: usize = segments
            .iter()
            .map(|s| s.end_sample.saturating_sub(s.start_sample))
            .sum();
        // 세그먼트 연결 사이에 80ms(1,280 샘플)의 미세 무음 간격을 두어 단어 뭉개짐 방지
        let inter_gap = 1_280usize;
        let mut trimmed_samples =
            Vec::with_capacity(total_trimmed_len + segments.len().saturating_sub(1) * inter_gap);

        for (i, seg) in segments.iter().enumerate() {
            if i > 0 {
                trimmed_samples.extend(std::iter::repeat_n(0.0f32, inter_gap));
            }
            trimmed_samples.extend_from_slice(&samples_16k[seg.start_sample..seg.end_sample]);
        }

        Ok(VadTrimResult {
            trimmed_samples,
            segments,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_silence_produces_zero_segments() {
        let probs = vec![0.02f32; 100];
        let segs = extract_speech_segments(&probs, 100 * VAD_WINDOW_SAMPLES);
        assert!(segs.is_empty());
    }

    #[test]
    fn test_hysteresis_merges_short_dip_and_extracts_speech() {
        let mut probs = vec![0.05f32; 120];
        // 10..40 구간 발화 (30 * 512 = 15,360 샘플 ≈ 0.96초)
        for p in &mut probs[10..40] {
            *p = 0.85;
        }
        // 중간 짧은 호흡 저하 (0.40은 NEG_SPEECH_THRESHOLD=0.35 위이므로 끊기지 않아야 함)
        probs[22] = 0.40;
        probs[23] = 0.38;

        let total = 120 * VAD_WINDOW_SAMPLES;
        let segs = extract_speech_segments(&probs, total);
        assert_eq!(segs.len(), 1);
        assert!(segs[0].start_sample <= 10 * VAD_WINDOW_SAMPLES);
        assert!(segs[0].end_sample >= 40 * VAD_WINDOW_SAMPLES);
    }
}
