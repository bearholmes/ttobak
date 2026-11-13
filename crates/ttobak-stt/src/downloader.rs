use anyhow::{anyhow, bail, Context, Result};
use futures_util::StreamExt;
use reqwest::header::RANGE;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use tokio::fs::{self, OpenOptions};
use tokio::io::AsyncWriteExt;
use tracing::info;
use ttobak_core::ModelVariant;

pub const SILERO_VAD_FILENAME: &str = "silero_vad_v6.onnx";
pub const SILERO_VAD_URL: &str =
    "https://raw.githubusercontent.com/SYSTRAN/faster-whisper/v1.2.1/faster_whisper/assets/silero_vad_v6.onnx";
pub const SILERO_VAD_SHA256: &str =
    "4cbf549b8326f60f80f2536d9eefeb450a9abe83365a098031c89719f1be17d2";

pub const TOKENIZER_FILENAME: &str = "tokenizer.json";
pub const TOKENIZER_URL: &str =
    "https://huggingface.co/openai/whisper-large-v3-turbo/resolve/main/tokenizer.json";

#[derive(Debug, Clone)]
pub struct ModelFiles {
    pub variant: ModelVariant,
    pub whisper_bin: PathBuf,
    pub silero_onnx: PathBuf,
    pub tokenizer_json: PathBuf,
}

#[derive(Debug, Clone)]
pub struct DownloadProgress {
    pub stage: String,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
    pub fraction: f32,
}

impl ModelFiles {
    pub fn in_dir(models_dir: &Path, variant: ModelVariant) -> Self {
        Self {
            variant,
            whisper_bin: models_dir.join(variant.filename()),
            silero_onnx: models_dir.join(SILERO_VAD_FILENAME),
            tokenizer_json: models_dir.join(TOKENIZER_FILENAME),
        }
    }

    pub fn are_core_models_present(&self) -> bool {
        let whisper_ok = std::fs::metadata(&self.whisper_bin)
            .map(|m| m.len() >= self.variant.min_valid_bytes())
            .unwrap_or(false);
        let vad_ok = std::fs::metadata(&self.silero_onnx)
            .map(|m| m.len() > 100_000)
            .unwrap_or(false);
        whisper_ok && vad_ok
    }
}

/// 선택된 모델 사양(`Fp16` 또는 `Q8_0`)과 공통 에셋(`silero_vad_v6.onnx`, `tokenizer.json`)을 이어받기 방식으로 내려받습니다.
pub async fn ensure_all_models_downloaded<F>(
    models_dir: &Path,
    variant: ModelVariant,
    mut on_progress: F,
) -> Result<ModelFiles>
where
    F: FnMut(DownloadProgress) + Send,
{
    fs::create_dir_all(models_dir).await?;
    let files = ModelFiles::in_dir(models_dir, variant);
    let client = reqwest::Client::builder()
        .user_agent("ttobak-macos/0.1.0")
        .build()?;

    // 1. Silero VAD v6 ONNX (~1.2MB, SHA-256 검증)
    if !verify_file_sha256_sync(&files.silero_onnx, SILERO_VAD_SHA256) {
        download_with_resume(
            &client,
            SILERO_VAD_URL,
            &files.silero_onnx,
            Some(SILERO_VAD_SHA256),
            "음성 감지 모델(Silero VAD v6) 준비 중",
            &mut on_progress,
        )
        .await?;
    }

    // 2. Whisper BPE 토크나이저 (~2.4MB)
    let tokenizer_ok = fs::metadata(&files.tokenizer_json)
        .await
        .map(|m| m.len() > 500_000)
        .unwrap_or(false);
    if !tokenizer_ok {
        download_with_resume(
            &client,
            TOKENIZER_URL,
            &files.tokenizer_json,
            None,
            "한국어 토크나이저 준비 중",
            &mut on_progress,
        )
        .await?;
    }

    // 3. 선택된 Whisper large-v3-turbo 모델 (FP16 ~1.62GB 또는 Q8_0 ~874MB)
    let whisper_ok = fs::metadata(&files.whisper_bin)
        .await
        .map(|m| m.len() >= variant.min_valid_bytes())
        .unwrap_or(false);
    if !whisper_ok {
        let stage_msg = match variant {
            ModelVariant::Fp16 => "Whisper large-v3-turbo 표준(FP16) 모델 다운로드 중",
            ModelVariant::Q8_0 => "Whisper large-v3-turbo 경량(Q8_0) 모델 다운로드 중",
        };
        download_with_resume(
            &client,
            variant.download_url(),
            &files.whisper_bin,
            None,
            stage_msg,
            &mut on_progress,
        )
        .await?;
    }

    info!("온디바이스 음성 모델({:?}) 준비 완료: {:?}", variant, models_dir);
    Ok(files)
}

fn verify_file_sha256_sync(path: &Path, expected_hex: &str) -> bool {
    let Ok(bytes) = std::fs::read(path) else {
        return false;
    };
    let digest = Sha256::digest(&bytes);
    hex::encode(digest).eq_ignore_ascii_case(expected_hex)
}

async fn download_with_resume<F>(
    client: &reqwest::Client,
    url: &str,
    dest: &Path,
    expected_sha256: Option<&str>,
    stage_label: &str,
    on_progress: &mut F,
) -> Result<()>
where
    F: FnMut(DownloadProgress) + Send,
{
    let part_path = PathBuf::from(format!("{}.part", dest.display()));
    let existing_len = fs::metadata(&part_path)
        .await
        .map(|m| m.len())
        .unwrap_or(0);

    let mut req = client.get(url);
    if existing_len > 0 {
        req = req.header(RANGE, format!("bytes={}-", existing_len));
    }

    let resp = req
        .send()
        .await
        .with_context(|| format!("다운로드 요청 실패: {}", url))?;

    let status = resp.status();
    if !status.is_success() && status.as_u16() != 206 {
        if existing_len > 0 && status.as_u16() == 416 {
            let _ = fs::remove_file(&part_path).await;
            return Box::pin(download_with_resume(
                client,
                url,
                dest,
                expected_sha256,
                stage_label,
                on_progress,
            ))
            .await;
        }
        bail!("모델 다운로드 서버 응답 오류 ({}): {}", status, url);
    }

    let is_partial = status.as_u16() == 206 && existing_len > 0;
    let content_len = resp.content_length().unwrap_or(0);
    let total_bytes = if is_partial {
        existing_len + content_len
    } else {
        content_len
    };

    let mut file = if is_partial {
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(&part_path)
            .await?
    } else {
        OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&part_path)
            .await?
    };

    let mut downloaded = if is_partial { existing_len } else { 0 };
    on_progress(DownloadProgress {
        stage: stage_label.to_string(),
        downloaded_bytes: downloaded,
        total_bytes,
        fraction: if total_bytes > 0 {
            (downloaded as f32 / total_bytes as f32).clamp(0.0, 1.0)
        } else {
            0.0
        },
    });

    let mut stream = resp.bytes_stream();
    let mut last_reported = downloaded;

    while let Some(chunk_res) = stream.next().await {
        let chunk = chunk_res.context("스트림 수신 중 네트워크 오류 발생")?;
        file.write_all(&chunk).await?;
        downloaded += chunk.len() as u64;

        if downloaded - last_reported >= 512 * 1024 || downloaded == total_bytes {
            last_reported = downloaded;
            on_progress(DownloadProgress {
                stage: stage_label.to_string(),
                downloaded_bytes: downloaded,
                total_bytes,
                fraction: if total_bytes > 0 {
                    (downloaded as f32 / total_bytes as f32).clamp(0.0, 1.0)
                } else {
                    0.0
                },
            });
        }
    }

    file.flush().await?;
    drop(file);

    if let Some(expected) = expected_sha256 {
        let bytes = fs::read(&part_path).await?;
        let actual = hex::encode(Sha256::digest(&bytes));
        if !actual.eq_ignore_ascii_case(expected) {
            let _ = fs::remove_file(&part_path).await;
            return Err(anyhow!(
                "다운로드한 파일 무결성(SHA-256) 불일치: 기대값={}, 실제값={}",
                expected,
                actual
            ));
        }
    }

    fs::rename(&part_path, dest).await?;
    Ok(())
}
