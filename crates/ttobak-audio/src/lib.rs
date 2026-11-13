use anyhow::{anyhow, Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use hound::{SampleFormat, WavSpec, WavWriter};
use parking_lot::Mutex;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{Duration, Instant};
use tokio::sync::watch;
use tracing::{info, warn};
use ttobak_core::MAX_RECORDING_DURATION_SECS;
use uuid::Uuid;

pub const TARGET_SAMPLE_RATE: u32 = 16_000;

/// 작업 종료(Drop) 시 임시 녹음 파일을 즉시 삭제하는 RAII 가드
pub struct ManagedRecording {
    pub path: PathBuf,
    pub samples_16k: Vec<f32>,
    pub duration_secs: f64,
}

impl Drop for ManagedRecording {
    fn drop(&mut self) {
        if self.path.exists() {
            if let Err(err) = fs::remove_file(&self.path) {
                warn!("임시 녹음 파일 삭제 실패 ({:?}): {}", self.path, err);
            }
        }
    }
}

/// 앱 시작 시 이전 비정상 종료 등으로 남은 임시 오디오 파일을 모두 정리합니다.
pub fn sweep_stale_recordings(temp_dir: &Path) -> Result<usize> {
    if !temp_dir.exists() {
        return Ok(0);
    }
    let mut removed = 0;
    for entry in fs::read_dir(temp_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() {
            if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
                if matches!(ext, "wav" | "tmp" | "pcm") {
                    if fs::remove_file(&path).is_ok() {
                        removed += 1;
                    }
                }
            }
        }
    }
    if removed > 0 {
        info!("시작 시 잔여 임시 녹음 파일 {}개 정리 완료", removed);
    }
    Ok(removed)
}

/// 임의 샘플레이트의 모노 PCM을 16kHz 모노 PCM으로 리샘플링합니다.
pub fn resample_mono_to_16k(input: &[f32], from_rate: u32) -> Vec<f32> {
    if input.is_empty() || from_rate == 0 {
        return Vec::new();
    }
    if from_rate == TARGET_SAMPLE_RATE {
        return input.to_vec();
    }

    let ratio = from_rate as f64 / TARGET_SAMPLE_RATE as f64;
    let out_len = ((input.len() as f64) / ratio).round() as usize;
    let mut out = Vec::with_capacity(out_len);

    // 다운샘플링 시 에일리어싱 방지를 위한 윈도우 평균 + 선형 보간 결합
    let half_window = (ratio * 0.5).ceil() as isize;
    for i in 0..out_len {
        let center = (i as f64) * ratio;
        if ratio > 1.25 && half_window >= 1 {
            let start = ((center - ratio * 0.5).floor() as isize).max(0) as usize;
            let end = ((center + ratio * 0.5).ceil() as usize).min(input.len());
            if end > start {
                let sum: f32 = input[start..end].iter().copied().sum();
                out.push(sum / (end - start) as f32);
                continue;
            }
        }
        let idx = center.floor() as usize;
        let frac = (center - idx as f64) as f32;
        let s0 = input[idx.min(input.len() - 1)];
        let s1 = input[(idx + 1).min(input.len() - 1)];
        out.push(s0 + (s1 - s0) * frac);
    }

    out
}

/// WAV 파일에서 16kHz 모노 f32 샘플을 로드합니다 (QA 테스트 및 외부 파일 검증용).
pub fn load_wav_mono_16k(path: &Path) -> Result<Vec<f32>> {
    let mut reader = hound::WavReader::open(path)
        .with_context(|| format!("WAV 파일 열기 실패: {:?}", path))?;
    let spec = reader.spec();
    let channels = spec.channels.max(1) as usize;

    let raw_mono: Vec<f32> = match spec.sample_format {
        SampleFormat::Float => {
            let samples: Vec<f32> = reader
                .samples::<f32>()
                .collect::<std::result::Result<Vec<_>, _>>()?;
            samples
                .chunks(channels)
                .map(|ch| ch.iter().copied().sum::<f32>() / channels as f32)
                .collect()
        }
        SampleFormat::Int => {
            let max_val = ((1i64 << (spec.bits_per_sample.saturating_sub(1))) as f32).max(1.0);
            let samples: Vec<i32> = reader
                .samples::<i32>()
                .collect::<std::result::Result<Vec<_>, _>>()?;
            samples
                .chunks(channels)
                .map(|ch| {
                    let avg = ch.iter().map(|&v| v as f32 / max_val).sum::<f32>() / channels as f32;
                    avg.clamp(-1.0, 1.0)
                })
                .collect()
        }
    };

    Ok(resample_mono_to_16k(&raw_mono, spec.sample_rate))
}

/// RMS 값을 HUD 파형에 적합한 [0.0, 1.0] 시각 레벨로 정규화합니다.
fn normalize_rms_for_hud(rms: f32) -> f32 {
    if rms <= 0.001 {
        return 0.0;
    }
    // 일반적인 말소리 RMS(0.01 ~ 0.18)가 잘 보이도록 로그 스케일 매핑
    let db = 20.0 * rms.log10();
    ((db + 50.0) / 42.0).clamp(0.0, 1.0)
}

enum WorkerCommand {
    Stop,
    Cancel,
}

/// 실시간 마이크 녹음 세션 핸들 (`cpal::Stream`을 전용 스레드에 격리)
pub struct ActiveAudioCapture {
    cmd_tx: mpsc::Sender<WorkerCommand>,
    result_rx: Mutex<Option<mpsc::Receiver<Result<ManagedRecording>>>>,
    pub level_rx: watch::Receiver<f32>,
    pub auto_stopped_rx: watch::Receiver<bool>,
    pub recording_id: String,
}

impl ActiveAudioCapture {
    pub fn start(temp_dir: PathBuf) -> Result<Self> {
        fs::create_dir_all(&temp_dir)?;
        let recording_id = Uuid::new_v4().to_string();
        let file_path = temp_dir.join(format!("rec_{}.wav", recording_id));

        let (cmd_tx, cmd_rx) = mpsc::channel::<WorkerCommand>();
        let (result_tx, result_rx) = mpsc::channel::<Result<ManagedRecording>>();
        let (startup_tx, startup_rx) = mpsc::channel::<Result<()>>();
        let (level_tx, level_rx) = watch::channel(0.0f32);
        let (auto_stop_tx, auto_stopped_rx) = watch::channel(false);

        thread::Builder::new()
            .name("ttobak-audio-capture".to_string())
            .spawn(move || {
                let host = cpal::default_host();
                let Some(device) = host.default_input_device() else {
                    let _ = startup_tx.send(Err(anyhow!("사용 가능한 마이크 장치를 찾을 수 없습니다.")));
                    return;
                };

                let supported = match device.default_input_config() {
                    Ok(cfg) => cfg,
                    Err(err) => {
                        let _ = startup_tx.send(Err(anyhow!("마이크 기본 설정 조회 실패: {}", err)));
                        return;
                    }
                };

                let sample_rate = supported.sample_rate().0;
                let channels = supported.channels() as usize;
                let config: cpal::StreamConfig = supported.clone().into();

                let raw_mono_buf = Arc::new(Mutex::new(Vec::<f32>::with_capacity(
                    (sample_rate as usize) * 30,
                )));
                let raw_clone = Arc::clone(&raw_mono_buf);
                let smoothed_level = Arc::new(Mutex::new(0.0f32));
                let smoothed_clone = Arc::clone(&smoothed_level);
                let limit_reached = Arc::new(AtomicBool::new(false));
                let limit_clone = Arc::clone(&limit_reached);
                let start_instant = Instant::now();
                let max_samples =
                    ((sample_rate as f64) * MAX_RECORDING_DURATION_SECS).ceil() as usize;

                let err_fn = |err| {
                    warn!("오디오 입력 스트림 오류: {}", err);
                };

                let push_frames = move |mono_chunk: &[f32]| {
                    if mono_chunk.is_empty() || limit_clone.load(Ordering::Relaxed) {
                        return;
                    }
                    // RMS 측정 및 지수 이동 평균 스무딩
                    let sq_sum: f32 = mono_chunk.iter().map(|&s| s * s).sum();
                    let rms = (sq_sum / mono_chunk.len() as f32).sqrt();
                    let target_lvl = normalize_rms_for_hud(rms);
                    {
                        let mut prev = smoothed_clone.lock();
                        let alpha = if target_lvl > *prev { 0.45 } else { 0.18 };
                        *prev = *prev * (1.0 - alpha) + target_lvl * alpha;
                        let _ = level_tx.send(*prev);
                    }

                    let mut guard = raw_clone.lock();
                    let remaining = max_samples.saturating_sub(guard.len());
                    let take_n = remaining.min(mono_chunk.len());
                    guard.extend_from_slice(&mono_chunk[..take_n]);
                    if guard.len() >= max_samples {
                        limit_clone.store(true, Ordering::Relaxed);
                        let _ = auto_stop_tx.send(true);
                    }
                };

                let stream_res = match supported.sample_format() {
                    cpal::SampleFormat::F32 => {
                        let cb = push_frames;
                        device.build_input_stream(
                            &config,
                            move |data: &[f32], _| {
                                let mono: Vec<f32> = data
                                    .chunks(channels.max(1))
                                    .map(|c| c.iter().copied().sum::<f32>() / channels.max(1) as f32)
                                    .collect();
                                cb(&mono);
                            },
                            err_fn,
                            None,
                        )
                    }
                    cpal::SampleFormat::I16 => {
                        let cb = push_frames;
                        device.build_input_stream(
                            &config,
                            move |data: &[i16], _| {
                                let mono: Vec<f32> = data
                                    .chunks(channels.max(1))
                                    .map(|c| {
                                        c.iter().map(|&v| v as f32 / 32768.0).sum::<f32>()
                                            / channels.max(1) as f32
                                    })
                                    .collect();
                                cb(&mono);
                            },
                            err_fn,
                            None,
                        )
                    }
                    cpal::SampleFormat::U16 => {
                        let cb = push_frames;
                        device.build_input_stream(
                            &config,
                            move |data: &[u16], _| {
                                let mono: Vec<f32> = data
                                    .chunks(channels.max(1))
                                    .map(|c| {
                                        c.iter()
                                            .map(|&v| (v as f32 - 32768.0) / 32768.0)
                                            .sum::<f32>()
                                            / channels.max(1) as f32
                                    })
                                    .collect();
                                cb(&mono);
                            },
                            err_fn,
                            None,
                        )
                    }
                    _other => Err(cpal::BuildStreamError::StreamConfigNotSupported),
                };

                let stream = match stream_res {
                    Ok(s) => s,
                    Err(err) => {
                        let _ = startup_tx.send(Err(anyhow!("마이크 스트림 생성 실패: {}", err)));
                        return;
                    }
                };

                if let Err(err) = stream.play() {
                    let _ = startup_tx.send(Err(anyhow!("마이크 녹음 시작 실패: {}", err)));
                    return;
                }

                let _ = startup_tx.send(Ok(()));

                // 종료 명령을 대기 (최대 10분 안전 타임아웃 포함)
                let cmd = loop {
                    match cmd_rx.recv_timeout(Duration::from_millis(100)) {
                        Ok(c) => break c,
                        Err(mpsc::RecvTimeoutError::Timeout) => {
                            if limit_reached.load(Ordering::Relaxed)
                                || start_instant.elapsed().as_secs_f64()
                                    >= MAX_RECORDING_DURATION_SECS + 1.0
                            {
                                // 상위 코디네이터가 stop()을 호출할 때까지 스트림만 일시정지 상태로 대기
                                let _ = stream.pause();
                            }
                        }
                        Err(mpsc::RecvTimeoutError::Disconnected) => break WorkerCommand::Cancel,
                    }
                };

                let _ = stream.pause();
                drop(stream);

                match cmd {
                    WorkerCommand::Cancel => {
                        if file_path.exists() {
                            let _ = fs::remove_file(&file_path);
                        }
                        let _ = result_tx.send(Err(anyhow!("녹음이 취소되었습니다.")));
                    }
                    WorkerCommand::Stop => {
                        let raw = std::mem::take(&mut *raw_mono_buf.lock());
                        let duration_secs = (raw.len() as f64) / (sample_rate as f64).max(1.0);
                        let samples_16k = resample_mono_to_16k(&raw, sample_rate);

                        // 임시 WAV 파일 기록 (ManagedRecording Drop 시 자동 삭제됨)
                        let spec = WavSpec {
                            channels: 1,
                            sample_rate: TARGET_SAMPLE_RATE,
                            bits_per_sample: 16,
                            sample_format: SampleFormat::Int,
                        };
                        if let Ok(mut writer) = WavWriter::create(&file_path, spec) {
                            for &s in &samples_16k {
                                let q = (s.clamp(-1.0, 1.0) * 32767.0) as i16;
                                let _ = writer.write_sample(q);
                            }
                            let _ = writer.finalize();
                        }

                        let _ = result_tx.send(Ok(ManagedRecording {
                            path: file_path,
                            samples_16k,
                            duration_secs,
                        }));
                    }
                }
            })?;

        startup_rx
            .recv_timeout(Duration::from_secs(5))
            .context("마이크 초기화 시간 초과")??;

        Ok(Self {
            cmd_tx,
            result_rx: Mutex::new(Some(result_rx)),
            level_rx,
            auto_stopped_rx,
            recording_id,
        })
    }

    pub fn stop(&self) -> Result<ManagedRecording> {
        let _ = self.cmd_tx.send(WorkerCommand::Stop);
        let rx = self
            .result_rx
            .lock()
            .take()
            .ok_or_else(|| anyhow!("이미 종료된 녹음 세션입니다."))?;
        rx.recv_timeout(Duration::from_secs(10))
            .context("녹음 버퍼 마무리 시간 초과")?
    }

    pub fn cancel(&self) {
        let _ = self.cmd_tx.send(WorkerCommand::Cancel);
        if let Some(rx) = self.result_rx.lock().take() {
            let _ = rx.recv_timeout(Duration::from_secs(2));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resample_48k_to_16k_length_and_dc() {
        let input = vec![0.5f32; 48_000];
        let out = resample_mono_to_16k(&input, 48_000);
        assert_eq!(out.len(), 16_000);
        for v in out {
            assert!((v - 0.5).abs() < 1e-4);
        }
    }

    #[test]
    fn test_managed_recording_deletes_temp_file_on_drop() {
        let dir = std::env::temp_dir().join(format!("ttobak_test_{}", Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("rec_test.wav");
        fs::write(&file, b"dummy").unwrap();
        assert!(file.exists());
        {
            let _managed = ManagedRecording {
                path: file.clone(),
                samples_16k: vec![0.0; 160],
                duration_secs: 0.01,
            };
        }
        assert!(!file.exists());
        let _ = fs::remove_dir_all(&dir);
    }
}
