use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// 최대 연속 녹음 시간 (600.0초 = 10분)
pub const MAX_RECORDING_DURATION_SECS: f64 = 600.0;

/// Whisper BPE 토큰 기준 용어 사전 최대 한도 (80 토큰)
pub const KEYWORD_TOKEN_LIMIT: usize = 80;

/// 홀드 시간 프리셋 (밀리초): 즉시(0ms), 0.5초(500ms), 1.0초(1000ms)
pub const HOLD_PRESETS_MS: [u32; 3] = [0, 500, 1000];

/// 단축키 수식키 플래그
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ShortcutModifiers {
    pub command: bool,
    pub option: bool,
    pub control: bool,
    pub shift: bool,
}

/// 단축키 종류 (단독 수식키 및 일반 키 조합 모두 지원)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ShortcutKind {
    /// 기본값: 우측 Option (macOS keyCode = 61)
    RightOption,
    /// 좌측 Option (macOS keyCode = 58)
    LeftOption,
    /// 우측 Command (macOS keyCode = 54)
    RightCommand,
    /// 우측 Control (macOS keyCode = 62)
    RightControl,
    /// Fn / Globe 키 (macOS keyCode = 63)
    FnKey,
    /// 일반 조합 단축키 (예: Ctrl + Option + Space)
    KeyCombo {
        key_code: u16,
        modifiers: ShortcutModifiers,
        key_name: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShortcutConfig {
    pub kind: ShortcutKind,
    pub hold_ms: u32,
}

impl Default for ShortcutConfig {
    fn default() -> Self {
        Self {
            kind: ShortcutKind::RightOption,
            hold_ms: 0,
        }
    }
}

impl ShortcutConfig {
    pub fn display_label(&self) -> String {
        match &self.kind {
            ShortcutKind::RightOption => "우측 ⌥".to_string(),
            ShortcutKind::LeftOption => "좌측 ⌥".to_string(),
            ShortcutKind::RightCommand => "우측 ⌘".to_string(),
            ShortcutKind::RightControl => "우측 ⌃".to_string(),
            ShortcutKind::FnKey => "Fn".to_string(),
            ShortcutKind::KeyCombo {
                modifiers,
                key_name,
                ..
            } => {
                let mut parts = Vec::new();
                if modifiers.control {
                    parts.push("⌃");
                }
                if modifiers.option {
                    parts.push("⌥");
                }
                if modifiers.shift {
                    parts.push("⇧");
                }
                if modifiers.command {
                    parts.push("⌘");
                }
                parts.push(key_name.as_str());
                parts.join(" ")
            }
        }
    }
}

/// 온디바이스 Whisper 모델 사양 (표준 FP16 vs 경량 Q8_0)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelVariant {
    /// 기본 추천: 무손실 FP16 (`ggml-large-v3-turbo.bin`, ~1.62GB)
    #[default]
    Fp16,
    /// 경량 모드: 8비트 양자화 (`ggml-large-v3-turbo-q8_0.bin`, ~874MB)
    Q8_0,
}

impl ModelVariant {
    pub fn filename(&self) -> &'static str {
        match self {
            ModelVariant::Fp16 => "ggml-large-v3-turbo.bin",
            ModelVariant::Q8_0 => "ggml-large-v3-turbo-q8_0.bin",
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            ModelVariant::Fp16 => "whisper-large-v3-turbo (FP16 · Metal)",
            ModelVariant::Q8_0 => "whisper-large-v3-turbo (Q8_0 경량 · Metal)",
        }
    }

    pub fn download_url(&self) -> &'static str {
        match self {
            ModelVariant::Fp16 => {
                "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo.bin"
            }
            ModelVariant::Q8_0 => {
                "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo-q8_0.bin"
            }
        }
    }

    pub fn min_valid_bytes(&self) -> u64 {
        match self {
            ModelVariant::Fp16 => 1_500_000_000,
            ModelVariant::Q8_0 => 800_000_000,
        }
    }

    pub fn size_label(&self) -> &'static str {
        match self {
            ModelVariant::Fp16 => "~1.62GB",
            ModelVariant::Q8_0 => "~874MB",
        }
    }
}

/// 영구 저장되는 사용자 설정
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub shortcut: ShortcutConfig,
    pub launch_at_login: bool,
    pub keywords_raw: String,
    pub first_notice_accepted: bool,
    #[serde(default)]
    pub model_variant: ModelVariant,
    /// 추후 릴리스 배포 서버의 업데이트 매니페스트(`latest.json`) URL
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub update_feed_url: Option<String>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            shortcut: ShortcutConfig::default(),
            launch_at_login: false,
            keywords_raw: String::new(),
            first_notice_accepted: false,
            model_variant: ModelVariant::default(),
            update_feed_url: None,
        }
    }
}

/// 자주 쓰는 말 BPE 토큰 계산 결과
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeywordUsage {
    pub tokens: usize,
    pub limit: usize,
    pub remaining_tokens: usize,
    pub keyword_count: usize,
    pub usable_keyword_count: usize,
    pub usable_keywords: Vec<String>,
    pub is_over_limit: bool,
}

impl Default for KeywordUsage {
    fn default() -> Self {
        Self {
            tokens: 0,
            limit: KEYWORD_TOKEN_LIMIT,
            remaining_tokens: KEYWORD_TOKEN_LIMIT,
            keyword_count: 0,
            usable_keyword_count: 0,
            usable_keywords: Vec::new(),
            is_over_limit: false,
        }
    }
}

/// 권한 상태
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionState {
    Granted,
    Denied,
    NotDetermined,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionSnapshot {
    pub microphone: PermissionState,
    pub accessibility: PermissionState,
}

/// 온디바이스 STT 모델 준비 상태
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ModelReadiness {
    NotInstalled,
    Downloading {
        progress: f32,
        downloaded_bytes: u64,
        total_bytes: u64,
        stage: String,
    },
    Loading,
    Ready {
        model_name: String,
    },
    Error {
        message: String,
    },
}

/// 실시간 세션 상태 (HUD 및 메뉴 막대 동기화용)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "phase", rename_all = "snake_case")]
pub enum SessionPhase {
    Idle,
    FirstRunNotice,
    BlockedSecureInput {
        message: String,
    },
    Recording {
        recording_id: String,
        elapsed_secs: u32,
        rms_level: f32,
    },
    Transcribing {
        recording_id: String,
    },
    RecoveryCard {
        transcript: String,
        reason: String,
        remaining_fraction: f32,
    },
    Failed {
        message: String,
    },
}

/// 앱 버전 및 수동 업데이트 확인·적용 상태
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum AppUpdateState {
    #[default]
    Idle,
    Checking,
    UpToDate,
    Available {
        latest_version: String,
        download_url: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        release_notes: Option<String>,
    },
    Applying {
        latest_version: String,
    },
    Error {
        message: String,
    },
}

/// 기본 GitHub Releases 최신 릴리즈 API 엔드포인트
pub const DEFAULT_GITHUB_RELEASE_URL: &str =
    "https://api.github.com/repos/bearholmes/ttobak/releases/latest";

/// 원격 업데이트 매니페스트(`latest.json`) 응답 규격
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateManifest {
    pub version: String,
    pub download_url: String,
    #[serde(default)]
    pub release_notes: Option<String>,
}

/// GitHub Releases API 에셋 항목
#[derive(Debug, Clone, Deserialize)]
struct GitHubReleaseAsset {
    name: String,
    browser_download_url: String,
}

/// GitHub Releases API `/releases/latest` 응답 규격
#[derive(Debug, Clone, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    assets: Vec<GitHubReleaseAsset>,
}

/// GitHub 릴리즈 에셋 목록에서 현재 macOS 아키텍처에 가장 적합한 설치 파일 URL을 선택합니다.
fn select_macos_asset_url(assets: &[GitHubReleaseAsset], arch: &str) -> Option<String> {
    let arch_tokens: &[&str] = match arch {
        "aarch64" | "arm64" => &["aarch64", "arm64", "apple-silicon"],
        "x86_64" | "x64" => &["x86_64", "x64", "intel"],
        _ => &[],
    };

    let matches_arch = |name: &str| -> bool {
        let lower = name.to_ascii_lowercase();
        arch_tokens.iter().any(|token| lower.contains(token))
    };

    let pick_by_extensions = |exts: &[&str]| -> Option<String> {
        let matching: Vec<&GitHubReleaseAsset> = assets
            .iter()
            .filter(|asset| {
                let lower = asset.name.to_ascii_lowercase();
                exts.iter().any(|ext| lower.ends_with(ext))
            })
            .collect();

        matching
            .iter()
            .find(|asset| matches_arch(&asset.name))
            .or_else(|| matching.first())
            .map(|asset| asset.browser_download_url.clone())
    };

    pick_by_extensions(&[".dmg"])
        .or_else(|| pick_by_extensions(&[".pkg", ".app.tar.gz", ".zip"]))
}

/// 마크다운 링크 `[텍스트](URL)`에서 표시 텍스트만 남깁니다.
fn strip_markdown_links(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(open_bracket) = rest.find('[') {
        out.push_str(&rest[..open_bracket]);
        let after_bracket = &rest[open_bracket + 1..];
        if let Some(close_bracket) = after_bracket.find("](") {
            let label = &after_bracket[..close_bracket];
            let after_paren = &after_bracket[close_bracket + 2..];
            if let Some(close_paren) = after_paren.find(')') {
                out.push_str(label);
                rest = &after_paren[close_paren + 1..];
                continue;
            }
        }
        out.push('[');
        rest = after_bracket;
    }
    out.push_str(rest);
    out
}

/// 릴리즈 노트 본문에서 URL·보일러플레이트를 제외한 짧은 한 줄 요약을 추출합니다.
pub fn summarize_release_notes(raw: &str) -> Option<String> {
    for line in raw.lines() {
        let trimmed = line
            .trim()
            .trim_start_matches(['#', '-', '*', '>'])
            .trim();
        if trimmed.is_empty()
            || trimmed.chars().all(|c| c == '-' || c == '*' || c == '=')
        {
            continue;
        }

        let unlinked = strip_markdown_links(trimmed);
        let cleaned_tokens: Vec<&str> = unlinked
            .split_whitespace()
            .filter(|tok| {
                let t = tok.trim_matches(|c: char| matches!(c, '(' | ')' | '<' | '>' | ',' | ';'));
                !t.starts_with("http://") && !t.starts_with("https://")
            })
            .collect();
        let joined = cleaned_tokens
            .join(" ")
            .replace("**", "")
            .replace("__", "")
            .replace('`', "");
        let mut normalized = joined.trim().to_string();

        // GitHub 자동 생성 릴리즈 노트의 "by @author in" 꼬리표 제거
        if let Some(idx) = normalized.rfind(" by @") {
            normalized.truncate(idx);
        }
        let normalized = normalized.trim().trim_end_matches(':').trim();
        if normalized.is_empty() {
            continue;
        }

        let lower = normalized.to_ascii_lowercase();
        if lower == "what's changed"
            || lower.starts_with("full changelog")
            || lower.starts_with("new contributors")
        {
            continue;
        }

        let max_chars = 40;
        let summary = if normalized.chars().count() > max_chars {
            let prefix: String = normalized.chars().take(max_chars).collect();
            format!("{}…", prefix.trim_end())
        } else {
            normalized.to_string()
        };
        return Some(summary);
    }
    None
}

/// 커스텀 `latest.json` 또는 GitHub Releases API JSON 응답을 `UpdateManifest`로 변환합니다.
pub fn parse_update_manifest_payload(
    body: &str,
    arch: &str,
) -> std::result::Result<UpdateManifest, serde_json::Error> {
    if let Ok(manifest) = serde_json::from_str::<UpdateManifest>(body) {
        return Ok(manifest);
    }

    let release: GitHubRelease = serde_json::from_str(body)?;
    let download_url = select_macos_asset_url(&release.assets, arch).unwrap_or(release.html_url);
    let release_notes = release.body.as_deref().and_then(summarize_release_notes);

    Ok(UpdateManifest {
        version: release.tag_name,
        download_url,
        release_notes,
    })
}

/// SemVer(`major.minor.patch`) 기준으로 새 버전 여부를 판별합니다.
pub fn is_newer_semver(current: &str, candidate: &str) -> bool {
    fn parse_triple(raw: &str) -> Option<(u64, u64, u64)> {
        let trimmed = raw.trim().trim_start_matches(['v', 'V']);
        let core = trimmed.split(['-', '+']).next().unwrap_or(trimmed);
        let mut parts = core.split('.');
        let major = parts.next()?.parse().ok()?;
        let minor = parts.next().unwrap_or("0").parse().ok()?;
        let patch = parts.next().unwrap_or("0").parse().ok()?;
        Some((major, minor, patch))
    }

    match (parse_triple(current), parse_triple(candidate)) {
        (Some(cur), Some(cand)) => cand > cur,
        _ => false,
    }
}

/// 로컬 디스크에 보관 중인 음성 인식 모델 용량·설치 정보
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelStorageInfo {
    pub models_dir: String,
    pub fp16_installed: bool,
    pub fp16_bytes: u64,
    pub q8_0_installed: bool,
    pub q8_0_bytes: u64,
    pub total_bytes: u64,
}

/// 설정에서 요청하는 음성 모델 삭제 범위
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelDeleteTarget {
    Fp16,
    Q8_0,
    All,
}

/// 모델 디렉터리의 실제 파일 크기를 조사합니다.
pub fn inspect_models_dir(models_dir: &Path) -> ModelStorageInfo {
    let file_len = |name: &str| -> u64 {
        fs::metadata(models_dir.join(name))
            .map(|m| if m.is_file() { m.len() } else { 0 })
            .unwrap_or(0)
    };

    let fp16_main = file_len(ModelVariant::Fp16.filename());
    let fp16_part = file_len(&format!("{}.part", ModelVariant::Fp16.filename()));
    let q8_main = file_len(ModelVariant::Q8_0.filename());
    let q8_part = file_len(&format!("{}.part", ModelVariant::Q8_0.filename()));

    let mut total_bytes = 0_u64;
    if let Ok(entries) = fs::read_dir(models_dir) {
        for entry in entries.flatten() {
            if let Ok(meta) = entry.metadata() {
                if meta.is_file() {
                    total_bytes = total_bytes.saturating_add(meta.len());
                }
            }
        }
    }

    ModelStorageInfo {
        models_dir: models_dir.to_string_lossy().into_owned(),
        fp16_installed: fp16_main >= ModelVariant::Fp16.min_valid_bytes(),
        fp16_bytes: fp16_main.saturating_add(fp16_part),
        q8_0_installed: q8_main >= ModelVariant::Q8_0.min_valid_bytes(),
        q8_0_bytes: q8_main.saturating_add(q8_part),
        total_bytes,
    }
}

/// 지정한 범위의 온디바이스 모델 파일을 디스크에서 삭제합니다.
pub fn delete_models_in_dir(models_dir: &Path, target: ModelDeleteTarget) -> Result<()> {
    if !models_dir.exists() {
        return Ok(());
    }

    let remove_if_exists = |path: PathBuf| -> Result<()> {
        if path.exists() {
            fs::remove_file(&path)
                .with_context(|| format!("모델 파일 삭제 실패: {}", path.display()))?;
        }
        Ok(())
    };

    match target {
        ModelDeleteTarget::All => {
            for entry in fs::read_dir(models_dir)
                .with_context(|| format!("모델 폴더 조회 실패: {}", models_dir.display()))?
            {
                let entry = entry?;
                if entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
                    fs::remove_file(entry.path())?;
                }
            }
        }
        ModelDeleteTarget::Fp16 => {
            remove_if_exists(models_dir.join(ModelVariant::Fp16.filename()))?;
            remove_if_exists(models_dir.join(format!("{}.part", ModelVariant::Fp16.filename())))?;
        }
        ModelDeleteTarget::Q8_0 => {
            remove_if_exists(models_dir.join(ModelVariant::Q8_0.filename()))?;
            remove_if_exists(models_dir.join(format!("{}.part", ModelVariant::Q8_0.filename())))?;
        }
    }

    // 두 Whisper 모델이 모두 없어졌다면 공용 보조 파일(VAD·토크나이저·임시 조각)도 함께 정리
    let after = inspect_models_dir(models_dir);
    if after.fp16_bytes == 0 && after.q8_0_bytes == 0 && after.total_bytes > 0 {
        for entry in fs::read_dir(models_dir)? {
            let entry = entry?;
            if entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
                let _ = fs::remove_file(entry.path());
            }
        }
    }

    Ok(())
}

/// 프론트엔드 초기 로드 및 동기화용 전체 앱 스냅샷
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSnapshot {
    pub version: String,
    pub config: AppConfig,
    pub shortcut_label: String,
    pub keyword_usage: KeywordUsage,
    pub permissions: PermissionSnapshot,
    pub model_readiness: ModelReadiness,
    pub model_storage: ModelStorageInfo,
    pub app_update: AppUpdateState,
    pub session_phase: SessionPhase,
}

/// 앱 데이터 루트 경로 (`~/Library/Application Support/com.ttobak.app`)
pub struct AppPaths {
    pub root_dir: PathBuf,
    pub models_dir: PathBuf,
    pub temp_recordings_dir: PathBuf,
    pub config_file: PathBuf,
}

impl AppPaths {
    pub fn resolve() -> Result<Self> {
        let base = dirs::data_dir()
            .or_else(dirs::home_dir)
            .context("사용자 데이터 디렉터리를 찾을 수 없습니다")?;
        let root_dir = base.join("com.ttobak.app");
        let models_dir = root_dir.join("SpeechModels");
        let temp_recordings_dir = std::env::temp_dir().join("com.ttobak.app.recordings");
        let config_file = root_dir.join("config.json");

        fs::create_dir_all(&root_dir)?;
        fs::create_dir_all(&models_dir)?;
        fs::create_dir_all(&temp_recordings_dir)?;

        // 이전 빌드의 잔여 통계 파일이 있으면 정리
        let legacy_stats = root_dir.join("usage_stats.json");
        if legacy_stats.exists() {
            let _ = fs::remove_file(legacy_stats);
        }

        Ok(Self {
            root_dir,
            models_dir,
            temp_recordings_dir,
            config_file,
        })
    }

    pub fn load_config(&self) -> AppConfig {
        fs::read_to_string(&self.config_file)
            .ok()
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default()
    }

    pub fn save_config(&self, config: &AppConfig) -> Result<()> {
        let json = serde_json::to_string_pretty(config)?;
        fs::write(&self.config_file, json)?;
        Ok(())
    }
}

/// 쉼표 구분 자주 쓰는 말 파싱 유틸리티
pub fn parse_keyword_list(raw: &str) -> Vec<String> {
    let mut seen = Vec::<String>::new();
    for part in raw.split([',', '\n']) {
        let trimmed = part.trim();
        if !trimmed.is_empty() && !seen.iter().any(|existing| existing == trimmed) {
            seen.push(trimmed.to_string());
        }
    }
    seen
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_keyword_list_dedup_and_trim() {
        let list = parse_keyword_list(" 프뚝딱, 품의서 , gift-claw, 프뚝딱 , \n 또박 ");
        assert_eq!(list, vec!["프뚝딱", "품의서", "gift-claw", "또박"]);
    }

    #[test]
    fn test_default_shortcut_display() {
        let cfg = ShortcutConfig::default();
        assert_eq!(cfg.display_label(), "우측 ⌥");
    }

    #[test]
    fn test_model_variant_default_and_q8_0_metadata() {
        let cfg = AppConfig::default();
        assert_eq!(cfg.model_variant, ModelVariant::Fp16);
        assert_eq!(ModelVariant::Fp16.filename(), "ggml-large-v3-turbo.bin");
        assert_eq!(
            ModelVariant::Q8_0.filename(),
            "ggml-large-v3-turbo-q8_0.bin"
        );
        assert_eq!(ModelVariant::Q8_0.size_label(), "~874MB");
    }

    #[test]
    fn test_is_newer_semver() {
        assert!(is_newer_semver("0.1.0", "0.1.1"));
        assert!(is_newer_semver("0.1.0", "v0.2.0"));
        assert!(is_newer_semver("0.1.0", "1.0.0-rc1"));
        assert!(!is_newer_semver("0.1.0", "0.1.0"));
        assert!(!is_newer_semver("0.2.0", "0.1.9"));
    }

    #[test]
    fn test_parse_update_manifest_payload_supports_custom_and_github_release() {
        let custom = r#"{
            "version": "0.2.0",
            "download_url": "https://example.com/ttobak.dmg",
            "release_notes": "성능 개선"
        }"#;
        let parsed_custom = parse_update_manifest_payload(custom, "aarch64").unwrap();
        assert_eq!(parsed_custom.version, "0.2.0");
        assert_eq!(parsed_custom.download_url, "https://example.com/ttobak.dmg");
        assert_eq!(parsed_custom.release_notes.as_deref(), Some("성능 개선"));

        let github = r###"{
            "tag_name": "v0.3.0",
            "html_url": "https://github.com/bearholmes/ttobak/releases/tag/v0.3.0",
            "body": "## 변경 사항\n- 음성 인식 정확도 향상\n- 버그 수정",
            "assets": [
                {
                    "name": "ttobak_0.3.0_x64.dmg",
                    "browser_download_url": "https://github.com/bearholmes/ttobak/releases/download/v0.3.0/ttobak_0.3.0_x64.dmg"
                },
                {
                    "name": "ttobak_0.3.0_aarch64.dmg",
                    "browser_download_url": "https://github.com/bearholmes/ttobak/releases/download/v0.3.0/ttobak_0.3.0_aarch64.dmg"
                }
            ]
        }"###;
        let parsed_arm = parse_update_manifest_payload(github, "aarch64").unwrap();
        assert_eq!(parsed_arm.version, "v0.3.0");
        assert_eq!(
            parsed_arm.download_url,
            "https://github.com/bearholmes/ttobak/releases/download/v0.3.0/ttobak_0.3.0_aarch64.dmg"
        );
        assert_eq!(parsed_arm.release_notes.as_deref(), Some("변경 사항"));

        let github_no_assets = r#"{
            "tag_name": "v0.3.1",
            "html_url": "https://github.com/bearholmes/ttobak/releases/tag/v0.3.1",
            "body": "",
            "assets": []
        }"#;
        let parsed_fallback = parse_update_manifest_payload(github_no_assets, "aarch64").unwrap();
        assert_eq!(parsed_fallback.version, "v0.3.1");
        assert_eq!(
            parsed_fallback.download_url,
            "https://github.com/bearholmes/ttobak/releases/tag/v0.3.1"
        );
        assert_eq!(parsed_fallback.release_notes, None);
    }

    #[test]
    fn test_summarize_release_notes_strips_urls_and_boilerplate() {
        assert_eq!(
            summarize_release_notes(
                "**Full Changelog**: https://github.com/bearholmes/ttobak/compare/v0.9.0...v0.9.1"
            ),
            None
        );
        assert_eq!(
            summarize_release_notes(
                "## What's Changed\n* 보조 모니터 HUD 위치 개선 by @bearholmes in https://github.com/bearholmes/ttobak/pull/12\n\n**Full Changelog**: https://github.com/bearholmes/ttobak/compare/v0.9.0...v0.9.1"
            )
            .as_deref(),
            Some("보조 모니터 HUD 위치 개선")
        );
    }

    #[test]
    fn test_inspect_and_delete_models_in_dir() {
        let dir = std::env::temp_dir().join(format!(
            "ttobak_model_del_test_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        fs::write(dir.join(ModelVariant::Fp16.filename()), vec![1u8; 1024]).unwrap();
        fs::write(dir.join("silero_vad_v6.onnx"), vec![2u8; 512]).unwrap();

        let info = inspect_models_dir(&dir);
        assert_eq!(info.fp16_bytes, 1024);
        assert_eq!(info.total_bytes, 1536);

        delete_models_in_dir(&dir, ModelDeleteTarget::Fp16).unwrap();
        let after = inspect_models_dir(&dir);
        assert_eq!(after.total_bytes, 0);

        let _ = fs::remove_dir_all(&dir);
    }
}
