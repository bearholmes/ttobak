use anyhow::{anyhow, Result};
use parking_lot::Mutex;
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, LogicalPosition, Manager, PhysicalPosition};
use tracing::{error, info, warn};
use ttobak_audio::{sweep_stale_recordings, ActiveAudioCapture};
use ttobak_core::{
    delete_models_in_dir, inspect_models_dir, is_newer_semver, parse_update_manifest_payload,
    AppConfig, AppPaths, AppSnapshot, AppUpdateState, KeywordUsage, ModelDeleteTarget,
    ModelReadiness, ModelVariant, PermissionSnapshot, PermissionState, SessionPhase,
    ShortcutConfig, UpdateManifest, DEFAULT_GITHUB_RELEASE_URL,
};
use ttobak_macos::{
    hotkey::{GlobalHotkeyManager, HotkeyEvent},
    pasteboard::{copy_text_to_clipboard, deliver_transcript_preserving_clipboard, DeliveryOutcome},
    permissions::{
        check_microphone_permission, check_permissions, is_launch_at_login_enabled,
        open_system_privacy_settings, prompt_accessibility_permission,
        request_microphone_permission, set_launch_at_login,
    },
    security_focus::{focused_target_screen_point, inspect_focused_target, FocusStatus},
    window::{
        activate_macos_app, configure_floating_hud_window, current_external_frontmost_pid,
        hide_hud_without_activating, hud_origin_for_active_monitor,
        restore_external_app_focus_if_needed, show_hud_without_activating,
    },
};
use ttobak_stt::{
    downloader::{ensure_all_models_downloaded, ModelFiles},
    prompt::PromptTokenizer,
    SttPipeline,
};

pub const EVENT_SNAPSHOT: &str = "ttobak://snapshot";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HudDisplayMode {
    Hidden,
    PassiveOverlay,
    InteractiveCard,
}

pub struct SessionCoordinator {
    pub paths: AppPaths,
    config: Mutex<AppConfig>,
    model_readiness: Mutex<ModelReadiness>,
    app_update: Mutex<AppUpdateState>,
    last_permissions: Mutex<PermissionSnapshot>,
    session_phase: Mutex<SessionPhase>,
    hud_mode: Mutex<HudDisplayMode>,
    last_external_pid: Mutex<Option<i32>>,
    pipeline: Mutex<Option<Arc<SttPipeline>>>,
    standalone_tokenizer: Mutex<PromptTokenizer>,
    active_capture: Mutex<Option<Arc<ActiveAudioCapture>>>,
    hotkey_mgr: Mutex<Option<GlobalHotkeyManager>>,
    downloading_flag: AtomicBool,
    checking_update_flag: AtomicBool,
    recovery_epoch: AtomicU64,
    cancel_transcribe_epoch: AtomicU64,
}

impl SessionCoordinator {
    pub fn initialize() -> Result<Arc<Self>> {
        let paths = AppPaths::resolve()?;
        let _ = sweep_stale_recordings(&paths.temp_recordings_dir);

        let mut config = paths.load_config();
        config.launch_at_login = is_launch_at_login_enabled();

        let files = ModelFiles::in_dir(&paths.models_dir, config.model_variant);
        let standalone_tokenizer = PromptTokenizer::from_file_optional(&files.tokenizer_json);
        let initial_readiness = if files.are_core_models_present() {
            ModelReadiness::Loading
        } else {
            ModelReadiness::NotInstalled
        };

        Ok(Arc::new(Self {
            paths,
            config: Mutex::new(config),
            model_readiness: Mutex::new(initial_readiness),
            app_update: Mutex::new(AppUpdateState::Idle),
            last_permissions: Mutex::new(check_permissions()),
            session_phase: Mutex::new(SessionPhase::Idle),
            hud_mode: Mutex::new(HudDisplayMode::Hidden),
            last_external_pid: Mutex::new(None),
            pipeline: Mutex::new(None),
            standalone_tokenizer: Mutex::new(standalone_tokenizer),
            active_capture: Mutex::new(None),
            hotkey_mgr: Mutex::new(None),
            downloading_flag: AtomicBool::new(false),
            checking_update_flag: AtomicBool::new(false),
            recovery_epoch: AtomicU64::new(0),
            cancel_transcribe_epoch: AtomicU64::new(0),
        }))
    }

    pub fn bind_runtime(self: &Arc<Self>, app: AppHandle) {
        let initial_shortcut = self.config.lock().shortcut.clone();
        let coord_for_hotkey = Arc::clone(self);
        let app_for_hotkey = app.clone();

        let mgr = GlobalHotkeyManager::start(initial_shortcut, move |evt| {
            let c = Arc::clone(&coord_for_hotkey);
            let a = app_for_hotkey.clone();
            tauri::async_runtime::spawn(async move {
                match evt {
                    HotkeyEvent::ToggleTriggered => {
                        c.toggle_recording(&a).await;
                    }
                    HotkeyEvent::CancelTriggered => {
                        c.cancel_session(&a).await;
                    }
                }
            });
        });
        *self.hotkey_mgr.lock() = Some(mgr);

        // HUD 초기 네이티브 플로팅 레벨 및 위치 설정 (숨김 상태 유지)
        position_and_style_hud(&app, true);

        // macOS 시스템 설정에서 권한(마이크·손쉬운 사용)을 변경하면 새로고침을 누르지 않아도 즉시 감지하여 반영
        let coord_perm = Arc::clone(self);
        let app_perm = app.clone();
        tauri::async_runtime::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(1200));
            loop {
                interval.tick().await;
                let current = check_permissions();
                let changed = {
                    let mut prev = coord_perm.last_permissions.lock();
                    if *prev != current {
                        *prev = current;
                        true
                    } else {
                        false
                    }
                };
                if changed {
                    info!(
                        "권한 상태 변경 자동 감지: mic={:?}, accessibility={:?}",
                        current.microphone, current.accessibility
                    );
                    coord_perm.emit_snapshot(&app_perm);
                }
            }
        });

        // 디스크에 모델이 이미 존재하면 백그라운드에서 Metal 파이프라인 즉시 로드
        let initial_variant = self.config.lock().model_variant;
        let files = ModelFiles::in_dir(&self.paths.models_dir, initial_variant);
        if files.are_core_models_present() {
            let c = Arc::clone(self);
            let a = app.clone();
            tauri::async_runtime::spawn(async move {
                c.load_models_into_memory(&a, files).await;
            });
        } else {
            // 최초 실행 시 모델 안내를 위해 설정 창 자동 표시
            show_settings_window(&app);
        }
    }

    pub fn remember_frontmost_external_app(&self) {
        if let Some(pid) = current_external_frontmost_pid() {
            *self.last_external_pid.lock() = Some(pid);
        }
    }

    pub fn build_snapshot(&self) -> AppSnapshot {
        let config = self.config.lock().clone();
        let shortcut_label = config.shortcut.display_label();
        let keyword_usage = self.preview_keyword_usage(&config.keywords_raw);
        let permissions = check_permissions();
        *self.last_permissions.lock() = permissions;
        let model_readiness = self.model_readiness.lock().clone();
        let model_storage = inspect_models_dir(&self.paths.models_dir);
        let app_update = self.app_update.lock().clone();
        let session_phase = self.session_phase.lock().clone();

        AppSnapshot {
            version: env!("CARGO_PKG_VERSION").to_string(),
            config,
            shortcut_label,
            keyword_usage,
            permissions,
            model_readiness,
            model_storage,
            app_update,
            session_phase,
        }
    }

    pub fn preview_keyword_usage(&self, raw: &str) -> KeywordUsage {
        if let Some(pipe) = self.pipeline.lock().as_ref() {
            return pipe.tokenizer.analyze_keywords(raw);
        }
        self.standalone_tokenizer.lock().analyze_keywords(raw)
    }

    pub fn emit_snapshot(&self, app: &AppHandle) {
        let snap = self.build_snapshot();
        self.sync_hud_visibility(app, &snap.session_phase);
        let _ = app.emit(EVENT_SNAPSHOT, &snap);
    }

    fn set_phase(&self, app: &AppHandle, phase: SessionPhase) {
        // 녹음 중, 변환 중, 또는 복구 카드 표시 중일 때 전역 Esc 키 취소/닫기 활성화
        let is_esc_active = matches!(
            phase,
            SessionPhase::Recording { .. }
                | SessionPhase::Transcribing { .. }
                | SessionPhase::RecoveryCard { .. }
        );
        if let Some(mgr) = self.hotkey_mgr.lock().as_ref() {
            mgr.set_session_active(is_esc_active);
        }
        *self.session_phase.lock() = phase;
        self.emit_snapshot(app);
    }

    /// HUD 표시 모드가 실제로 바뀔 때만 창 표시/숨김을 호출하여,
    /// 65ms 파형 갱신 시 트레이 메뉴 팝오버나 외부 앱의 포커스가 뺏기지 않도록 보장합니다.
    fn sync_hud_visibility(&self, app: &AppHandle, phase: &SessionPhase) {
        let desired = match phase {
            SessionPhase::Idle | SessionPhase::FirstRunNotice => HudDisplayMode::Hidden,
            SessionPhase::RecoveryCard { .. } => HudDisplayMode::InteractiveCard,
            _ => HudDisplayMode::PassiveOverlay,
        };

        {
            let mut mode_guard = self.hud_mode.lock();
            if *mode_guard == desired {
                return;
            }
            *mode_guard = desired;
        }

        let Some(hud) = app.get_webview_window("hud") else {
            return;
        };

        match desired {
            HudDisplayMode::Hidden => {
                let hud_for_main = hud.clone();
                let _ = app.run_on_main_thread(move || {
                    if let Ok(ns_win) = hud_for_main.ns_window() {
                        unsafe { hide_hud_without_activating(ns_win) };
                    }
                });
            }
            HudDisplayMode::PassiveOverlay => {
                position_and_style_hud(app, true);
                let hud_for_main = hud.clone();
                let _ = app.run_on_main_thread(move || {
                    if let Ok(ns_win) = hud_for_main.ns_window() {
                        unsafe {
                            configure_floating_hud_window(ns_win, true);
                            show_hud_without_activating(ns_win);
                        }
                    }
                });
            }
            HudDisplayMode::InteractiveCard => {
                position_and_style_hud(app, false);
                let hud_for_main = hud.clone();
                let _ = app.run_on_main_thread(move || {
                    if let Ok(ns_win) = hud_for_main.ns_window() {
                        unsafe {
                            configure_floating_hud_window(ns_win, false);
                            show_hud_without_activating(ns_win);
                        }
                    }
                });
            }
        }
    }

    pub fn set_shortcut_recording_paused(&self, paused: bool) {
        if let Some(mgr) = self.hotkey_mgr.lock().as_ref() {
            mgr.set_paused(paused);
        }
    }

    pub fn update_shortcut(&self, app: &AppHandle, shortcut: ShortcutConfig) -> Result<()> {
        {
            let mut cfg = self.config.lock();
            cfg.shortcut = shortcut.clone();
            self.paths.save_config(&cfg)?;
        }
        if let Some(mgr) = self.hotkey_mgr.lock().as_ref() {
            mgr.update_config(shortcut);
        }
        self.emit_snapshot(app);
        Ok(())
    }

    pub fn update_keywords(&self, app: &AppHandle, keywords_raw: String) -> Result<KeywordUsage> {
        {
            let mut cfg = self.config.lock();
            cfg.keywords_raw = keywords_raw.clone();
            self.paths.save_config(&cfg)?;
        }
        let usage = self.preview_keyword_usage(&keywords_raw);
        self.emit_snapshot(app);
        Ok(usage)
    }

    pub fn update_launch_at_login(&self, app: &AppHandle, enabled: bool) -> Result<()> {
        set_launch_at_login(enabled)?;
        {
            let mut cfg = self.config.lock();
            cfg.launch_at_login = enabled;
            self.paths.save_config(&cfg)?;
        }
        self.emit_snapshot(app);
        Ok(())
    }

    pub fn accept_first_notice(&self, app: &AppHandle) -> Result<()> {
        {
            let mut cfg = self.config.lock();
            cfg.first_notice_accepted = true;
            self.paths.save_config(&cfg)?;
        }
        let reset_idle = matches!(*self.session_phase.lock(), SessionPhase::FirstRunNotice);
        if reset_idle {
            *self.session_phase.lock() = SessionPhase::Idle;
        }
        self.emit_snapshot(app);
        Ok(())
    }

    pub async fn check_for_app_update(self: &Arc<Self>, app: &AppHandle) {
        if self
            .checking_update_flag
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return;
        }

        *self.app_update.lock() = AppUpdateState::Checking;
        self.emit_snapshot(app);

        let feed_url = std::env::var("TTOBAK_UPDATE_MANIFEST_URL")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .or_else(|| {
                self.config
                    .lock()
                    .update_feed_url
                    .clone()
                    .filter(|s| !s.trim().is_empty())
            })
            .unwrap_or_else(|| DEFAULT_GITHUB_RELEASE_URL.to_string());

        let next_state = match fetch_remote_update_manifest(&feed_url).await {
            Ok(Some(manifest)) => {
                let current_ver = env!("CARGO_PKG_VERSION");
                if is_newer_semver(current_ver, &manifest.version) {
                    AppUpdateState::Available {
                        latest_version: manifest
                            .version
                            .trim()
                            .trim_start_matches(['v', 'V'])
                            .to_string(),
                        download_url: manifest.download_url,
                        release_notes: manifest.release_notes,
                    }
                } else {
                    AppUpdateState::UpToDate
                }
            }
            Ok(None) => {
                // GitHub Releases에 아직 게시된 릴리즈가 없는 경우 최신 버전으로 간주
                AppUpdateState::UpToDate
            }
            Err(err) => {
                warn!("앱 업데이트 확인 실패: {:#}", err);
                let raw = err.to_string();
                let message = if raw.contains("GitHub API 요청 한도") {
                    raw
                } else {
                    "업데이트 확인에 실패했습니다. 잠시 후 다시 시도해주세요.".to_string()
                };
                AppUpdateState::Error { message }
            }
        };

        *self.app_update.lock() = next_state;
        self.checking_update_flag.store(false, Ordering::SeqCst);
        self.emit_snapshot(app);
    }

    pub async fn apply_app_update(self: &Arc<Self>, app: &AppHandle) -> Result<()> {
        let available = match self.app_update.lock().clone() {
            AppUpdateState::Available {
                latest_version,
                download_url,
                release_notes,
            } => Some((latest_version, download_url, release_notes)),
            _ => None,
        };

        let Some((latest_version, download_url, release_notes)) = available else {
            return Err(anyhow!("현재 적용 가능한 새 업데이트가 없습니다."));
        };

        if !download_url.starts_with("https://") && !download_url.starts_with("http://") {
            return Err(anyhow!("유효하지 않은 업데이트 다운로드 주소입니다."));
        }

        *self.app_update.lock() = AppUpdateState::Applying {
            latest_version: latest_version.clone(),
        };
        self.emit_snapshot(app);

        if is_direct_installer_url(&download_url) {
            match download_and_open_installer(&download_url, &latest_version).await {
                Ok(()) => {
                    *self.app_update.lock() = AppUpdateState::Available {
                        latest_version,
                        download_url,
                        release_notes,
                    };
                    self.emit_snapshot(app);
                    return Ok(());
                }
                Err(err) => {
                    warn!("설치 파일 직접 다운로드 실패, 브라우저 열기로 전환: {:#}", err);
                    // 직접 다운로드 실패 시에도 사용자가 릴리즈 에셋 URL을 브라우저로 열 수 있도록 fallback
                    if let Err(open_err) = Command::new("open").arg(&download_url).spawn() {
                        let msg = format!("업데이트 설치 파일 열기 실패: {}", open_err);
                        *self.app_update.lock() = AppUpdateState::Error {
                            message: msg.clone(),
                        };
                        self.emit_snapshot(app);
                        return Err(anyhow!(msg));
                    }
                }
            }
        } else {
            Command::new("open")
                .arg(&download_url)
                .spawn()
                .map_err(|e| anyhow!("업데이트 페이지 열기 실패: {}", e))?;
        }

        let coord = Arc::clone(self);
        let app_clone = app.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(Duration::from_millis(1200)).await;
            let mut guard = coord.app_update.lock();
            if matches!(&*guard, AppUpdateState::Applying { .. }) {
                *guard = AppUpdateState::Available {
                    latest_version,
                    download_url,
                    release_notes,
                };
                drop(guard);
                coord.emit_snapshot(&app_clone);
            }
        });

        Ok(())
    }

    pub async fn update_model_variant(
        self: &Arc<Self>,
        app: &AppHandle,
        variant: ModelVariant,
    ) -> Result<()> {
        if self.downloading_flag.load(Ordering::SeqCst) {
            anyhow::bail!("모델 다운로드가 진행 중일 때는 사양을 변경할 수 없어요.");
        }
        if matches!(
            *self.session_phase.lock(),
            SessionPhase::Recording { .. } | SessionPhase::Transcribing { .. }
        ) {
            anyhow::bail!("녹음이나 변환 중에는 모델을 변경할 수 없어요.");
        }

        let changed = {
            let mut cfg = self.config.lock();
            if cfg.model_variant == variant {
                false
            } else {
                cfg.model_variant = variant;
                self.paths.save_config(&cfg)?;
                true
            }
        };

        if !changed && matches!(*self.model_readiness.lock(), ModelReadiness::Ready { .. }) {
            return Ok(());
        }

        // 기존 로드된 모델 메모리를 즉시 해제하여 RAM 중복 점유 방지
        *self.pipeline.lock() = None;

        let files = ModelFiles::in_dir(&self.paths.models_dir, variant);
        if files.are_core_models_present() {
            self.load_models_into_memory(app, files).await;
        } else {
            *self.model_readiness.lock() = ModelReadiness::NotInstalled;
            self.emit_snapshot(app);
        }
        Ok(())
    }

    pub fn delete_models(&self, app: &AppHandle, target: ModelDeleteTarget) -> Result<()> {
        if self.downloading_flag.load(Ordering::SeqCst) {
            anyhow::bail!("모델 다운로드가 진행 중일 때는 삭제할 수 없어요.");
        }
        if matches!(
            *self.session_phase.lock(),
            SessionPhase::Recording { .. } | SessionPhase::Transcribing { .. }
        ) {
            anyhow::bail!("녹음이나 변환 중에는 모델을 삭제할 수 없어요.");
        }

        let current_variant = self.config.lock().model_variant;
        let affects_current = matches!(
            (target, current_variant),
            (ModelDeleteTarget::All, _)
                | (ModelDeleteTarget::Fp16, ModelVariant::Fp16)
                | (ModelDeleteTarget::Q8_0, ModelVariant::Q8_0)
        );

        if affects_current {
            // 파일 삭제 전 메모리 및 Metal GPU 컨텍스트 즉시 해제
            *self.pipeline.lock() = None;
            *self.model_readiness.lock() = ModelReadiness::NotInstalled;
        }

        delete_models_in_dir(&self.paths.models_dir, target)?;

        let tok_path = self.paths.models_dir.join("tokenizer.json");
        *self.standalone_tokenizer.lock() = PromptTokenizer::from_file_optional(&tok_path);

        info!("온디바이스 음성 모델 삭제 완료: {:?}", target);
        self.emit_snapshot(app);
        Ok(())
    }

    pub fn reveal_models_folder(&self) -> Result<()> {
        std::fs::create_dir_all(&self.paths.models_dir)?;
        Command::new("open")
            .arg(&self.paths.models_dir)
            .spawn()
            .map_err(|e| anyhow!("모델 보관 폴더 열기 실패: {}", e))?;
        Ok(())
    }

    pub async fn start_model_download(self: &Arc<Self>, app: &AppHandle) {
        if self
            .downloading_flag
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return;
        }

        let variant = self.config.lock().model_variant;
        let models_dir = self.paths.models_dir.clone();
        let coord_prog = Arc::clone(self);
        let app_prog = app.clone();

        let res = ensure_all_models_downloaded(&models_dir, variant, move |p| {
            *coord_prog.model_readiness.lock() = ModelReadiness::Downloading {
                progress: p.fraction,
                downloaded_bytes: p.downloaded_bytes,
                total_bytes: p.total_bytes,
                stage: p.stage,
            };
            coord_prog.emit_snapshot(&app_prog);
        })
        .await;

        self.downloading_flag.store(false, Ordering::SeqCst);

        match res {
            Ok(files) => {
                *self.standalone_tokenizer.lock() =
                    PromptTokenizer::from_file_optional(&files.tokenizer_json);
                self.load_models_into_memory(app, files).await;
            }
            Err(err) => {
                error!("모델 다운로드 실패: {:#}", err);
                *self.model_readiness.lock() = ModelReadiness::Error {
                    message: format!("모델 다운로드 실패: {}", err),
                };
                self.emit_snapshot(app);
            }
        }
    }

    async fn load_models_into_memory(self: &Arc<Self>, app: &AppHandle, files: ModelFiles) {
        *self.model_readiness.lock() = ModelReadiness::Loading;
        self.emit_snapshot(app);

        let display_name = files.variant.display_name().to_string();
        let load_res = tokio::task::spawn_blocking(move || SttPipeline::load(&files)).await;
        match load_res {
            Ok(Ok(pipe)) => {
                *self.pipeline.lock() = Some(Arc::new(pipe));
                *self.model_readiness.lock() = ModelReadiness::Ready {
                    model_name: display_name,
                };
                info!("온디바이스 STT 파이프라인 준비 완료");
            }
            Ok(Err(err)) => {
                error!("STT 모델 메모리 로드 실패: {:#}", err);
                *self.model_readiness.lock() = ModelReadiness::Error {
                    message: format!("모델 로드 실패: {}", err),
                };
            }
            Err(join_err) => {
                *self.model_readiness.lock() = ModelReadiness::Error {
                    message: format!("모델 로더 작업 중단: {}", join_err),
                };
            }
        }
        self.emit_snapshot(app);
    }

    pub async fn toggle_recording(self: &Arc<Self>, app: &AppHandle) {
        // 이미 녹음 중이면 메뉴 팝오버를 닫고 변환 단계로 진입
        let existing_capture = self.active_capture.lock().take();
        if let Some(capture) = existing_capture {
            if let Some(menu_win) = app.get_webview_window("menu") {
                let _ = menu_win.hide();
            }
            self.finish_recording_and_transcribe(app, capture).await;
            return;
        }

        // 변환 중에는 중복 토글 무시
        if matches!(*self.session_phase.lock(), SessionPhase::Transcribing { .. }) {
            return;
        }

        // 녹음 시작 전 현재 작업 중이던 외부 앱 기억
        self.remember_frontmost_external_app();

        // 이전 복구 카드가 떠 있었다면 닫고 새 세션 준비
        self.recovery_epoch.fetch_add(1, Ordering::SeqCst);

        // 1. 첫 사용 안내 확인
        if !self.config.lock().first_notice_accepted {
            self.set_phase(app, SessionPhase::FirstRunNotice);
            show_settings_window(app);
            return;
        }

        // 2. 모델 준비 여부 확인
        let is_model_ready = matches!(*self.model_readiness.lock(), ModelReadiness::Ready { .. });
        if !is_model_ready {
            show_settings_window(app);
            if matches!(*self.model_readiness.lock(), ModelReadiness::NotInstalled) {
                let c = Arc::clone(self);
                let a = app.clone();
                tauri::async_runtime::spawn(async move {
                    c.start_model_download(&a).await;
                });
            }
            return;
        }

        // 3. 마이크 권한 확인 및 요청
        let mic_state = match check_microphone_permission() {
            PermissionState::NotDetermined => {
                tokio::task::spawn_blocking(request_microphone_permission)
                    .await
                    .unwrap_or(PermissionState::Denied)
            }
            other => other,
        };
        if mic_state != PermissionState::Granted {
            self.flash_transient_phase(
                app,
                SessionPhase::Failed {
                    message: "마이크 권한이 필요해요. 설정 창에서 허용해 주세요.".to_string(),
                },
                Duration::from_millis(2600),
            );
            show_settings_window(app);
            return;
        }

        // 4. 녹음 시작 전 1차 비밀번호 입력란 차단 검사
        if let FocusStatus::SecureInputBlocked { reason } = inspect_focused_target() {
            self.flash_transient_phase(
                app,
                SessionPhase::BlockedSecureInput { message: reason },
                Duration::from_millis(2200),
            );
            return;
        }

        // 5. 실시간 마이크 캡처 시작
        let temp_dir = self.paths.temp_recordings_dir.clone();
        let capture = match ActiveAudioCapture::start(temp_dir) {
            Ok(cap) => Arc::new(cap),
            Err(err) => {
                warn!("마이크 녹음 시작 실패: {:#}", err);
                self.flash_transient_phase(
                    app,
                    SessionPhase::Failed {
                        message: format!("마이크를 시작할 수 없어요: {}", err),
                    },
                    Duration::from_millis(2600),
                );
                return;
            }
        };

        let rec_id = capture.recording_id.clone();
        let level_rx = capture.level_rx.clone();
        let auto_stop_rx = capture.auto_stopped_rx.clone();
        *self.active_capture.lock() = Some(Arc::clone(&capture));

        // 메뉴 팝오버에서 시작한 경우 직전 외부 앱으로 포커스 복귀 후 팝오버 닫기
        if let Some(menu_win) = app.get_webview_window("menu") {
            if menu_win.is_visible().unwrap_or(false) {
                let _ = menu_win.hide();
                let saved_pid = *self.last_external_pid.lock();
                let _ = restore_external_app_focus_if_needed(saved_pid);
            }
        }

        self.set_phase(
            app,
            SessionPhase::Recording {
                recording_id: rec_id.clone(),
                elapsed_secs: 0,
                rms_level: 0.0,
            },
        );

        // 실시간 HUD 파형/타이머 업데이트 및 10분(600초) 자동 종료 감시 루프
        let coord_tick = Arc::clone(self);
        let app_tick = app.clone();
        let started_at = Instant::now();

        tauri::async_runtime::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(65));
            loop {
                interval.tick().await;
                let still_same = coord_tick
                    .active_capture
                    .lock()
                    .as_ref()
                    .map(|c| c.recording_id == rec_id)
                    .unwrap_or(false);
                if !still_same {
                    break;
                }

                if *auto_stop_rx.borrow() {
                    info!("최대 녹음 시간(10분) 도달 -> 자동 변환 진입");
                    let taken_cap = coord_tick.active_capture.lock().take();
                    if let Some(cap) = taken_cap {
                        coord_tick
                            .finish_recording_and_transcribe(&app_tick, cap)
                            .await;
                    }
                    break;
                }

                let lvl = *level_rx.borrow();
                let elapsed = started_at.elapsed().as_secs() as u32;
                coord_tick.set_phase(
                    &app_tick,
                    SessionPhase::Recording {
                        recording_id: rec_id.clone(),
                        elapsed_secs: elapsed,
                        rms_level: lvl,
                    },
                );
            }
        });
    }

    pub async fn cancel_session(self: &Arc<Self>, app: &AppHandle) {
        info!("음성 입력 세션 취소 요청 처리 (Esc / 메뉴 취소)");
        self.cancel_transcribe_epoch.fetch_add(1, Ordering::SeqCst);
        self.recovery_epoch.fetch_add(1, Ordering::SeqCst);

        if let Some(capture) = self.active_capture.lock().take() {
            capture.cancel();
        }
        if let Some(menu_win) = app.get_webview_window("menu") {
            if menu_win.is_visible().unwrap_or(false) {
                let _ = menu_win.hide();
                let saved_pid = *self.last_external_pid.lock();
                let _ = restore_external_app_focus_if_needed(saved_pid);
            }
        }
        self.set_phase(app, SessionPhase::Idle);
    }

    async fn finish_recording_and_transcribe(
        self: &Arc<Self>,
        app: &AppHandle,
        capture: Arc<ActiveAudioCapture>,
    ) {
        let rec_id = capture.recording_id.clone();
        let my_epoch = self.cancel_transcribe_epoch.load(Ordering::SeqCst);
        self.set_phase(
            app,
            SessionPhase::Transcribing {
                recording_id: rec_id,
            },
        );

        let stop_res = tokio::task::spawn_blocking(move || capture.stop()).await;
        let managed_rec = match stop_res {
            Ok(Ok(rec)) => rec,
            Ok(Err(err)) => {
                warn!("녹음 종료 실패: {}", err);
                self.set_phase(app, SessionPhase::Idle);
                return;
            }
            Err(_) => {
                self.set_phase(app, SessionPhase::Idle);
                return;
            }
        };

        if self.cancel_transcribe_epoch.load(Ordering::SeqCst) != my_epoch {
            drop(managed_rec);
            self.set_phase(app, SessionPhase::Idle);
            return;
        }

        let Some(pipeline) = self.pipeline.lock().clone() else {
            drop(managed_rec);
            self.set_phase(app, SessionPhase::Idle);
            return;
        };

        let keywords_raw = self.config.lock().keywords_raw.clone();

        let stt_res = tokio::task::spawn_blocking(move || {
            let outcome = pipeline.transcribe_samples(&managed_rec.samples_16k, &keywords_raw);
            // 추론 직후 ManagedRecording을 명시적으로 Drop하여 임시 WAV 파일을 즉시 삭제
            drop(managed_rec);
            outcome
        })
        .await;

        if self.cancel_transcribe_epoch.load(Ordering::SeqCst) != my_epoch {
            self.set_phase(app, SessionPhase::Idle);
            return;
        }

        let outcome = match stt_res {
            Ok(Ok(out)) => out,
            Ok(Err(err)) => {
                error!("음성 인식 추론 실패: {:#}", err);
                self.flash_transient_phase(
                    app,
                    SessionPhase::Failed {
                        message: format!("글자로 바꾸는 중 문제가 생겼어요: {}", err),
                    },
                    Duration::from_millis(2600),
                );
                return;
            }
            Err(join_err) => {
                error!("추론 작업 스레드 오류: {}", join_err);
                self.set_phase(app, SessionPhase::Idle);
                return;
            }
        };

        let transcript = outcome.transcript.trim().to_string();
        if transcript.is_empty() {
            // 무음/잡음만 있었던 경우 조용히 대기 상태로 복귀
            self.set_phase(app, SessionPhase::Idle);
            return;
        }

        // 트레이 메뉴를 눌러 완료한 경우 직전 외부 앱으로 포커스 복귀 대기
        let saved_pid = *self.last_external_pid.lock();
        if restore_external_app_focus_if_needed(saved_pid) {
            tokio::time::sleep(Duration::from_millis(85)).await;
        }

        // 커서 위치 입력 시도 (내부에서 2차 비밀번호 칸 재검사 + 클립보드 스냅샷 백업/복원 수행)
        let text_for_paste = transcript.clone();
        let delivery = tokio::task::spawn_blocking(move || {
            deliver_transcript_preserving_clipboard(&text_for_paste)
        })
        .await
        .unwrap_or(DeliveryOutcome::NeedsRecoveryCard {
            reason: "입력 처리 중 오류가 발생해 카드로 결과를 보여드려요.".to_string(),
        });

        match delivery {
            DeliveryOutcome::PastedAtCursor => {
                self.set_phase(app, SessionPhase::Idle);
            }
            DeliveryOutcome::BlockedSecure { reason } => {
                self.flash_transient_phase(
                    app,
                    SessionPhase::BlockedSecureInput { message: reason },
                    Duration::from_millis(2500),
                );
            }
            DeliveryOutcome::NeedsRecoveryCard { reason } => {
                self.present_recovery_card(app, transcript, reason);
            }
        }
    }

    fn present_recovery_card(
        self: &Arc<Self>,
        app: &AppHandle,
        transcript: String,
        reason: String,
    ) {
        let epoch = self.recovery_epoch.fetch_add(1, Ordering::SeqCst) + 1;
        let total_dur = Duration::from_secs(16);
        let started = Instant::now();

        self.set_phase(
            app,
            SessionPhase::RecoveryCard {
                transcript: transcript.clone(),
                reason: reason.clone(),
                remaining_fraction: 1.0,
            },
        );

        let coord = Arc::clone(self);
        let app_clone = app.clone();
        tauri::async_runtime::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(120));
            loop {
                interval.tick().await;
                if coord.recovery_epoch.load(Ordering::SeqCst) != epoch {
                    break;
                }
                let elapsed = started.elapsed();
                if elapsed >= total_dur {
                    coord.set_phase(&app_clone, SessionPhase::Idle);
                    break;
                }
                let rem = 1.0 - (elapsed.as_secs_f32() / total_dur.as_secs_f32()).clamp(0.0, 1.0);
                coord.set_phase(
                    &app_clone,
                    SessionPhase::RecoveryCard {
                        transcript: transcript.clone(),
                        reason: reason.clone(),
                        remaining_fraction: rem,
                    },
                );
            }
        });
    }

    pub fn copy_recovery_and_dismiss(&self, app: &AppHandle) -> Result<()> {
        let maybe_text = match &*self.session_phase.lock() {
            SessionPhase::RecoveryCard { transcript, .. } => Some(transcript.clone()),
            _ => None,
        };
        if let Some(text) = maybe_text {
            copy_text_to_clipboard(&text)?;
        }
        self.recovery_epoch.fetch_add(1, Ordering::SeqCst);
        self.set_phase(app, SessionPhase::Idle);
        Ok(())
    }

    pub fn dismiss_recovery(&self, app: &AppHandle) {
        self.recovery_epoch.fetch_add(1, Ordering::SeqCst);
        self.set_phase(app, SessionPhase::Idle);
    }

    fn flash_transient_phase(
        self: &Arc<Self>,
        app: &AppHandle,
        phase: SessionPhase,
        hold: Duration,
    ) {
        let epoch = self.recovery_epoch.fetch_add(1, Ordering::SeqCst) + 1;
        self.set_phase(app, phase);
        let coord = Arc::clone(self);
        let app_clone = app.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(hold).await;
            if coord.recovery_epoch.load(Ordering::SeqCst) == epoch {
                coord.set_phase(&app_clone, SessionPhase::Idle);
            }
        });
    }

    pub fn request_or_open_permission(&self, app: &AppHandle, kind: &str) -> Result<()> {
        match kind {
            "microphone" => {
                if check_microphone_permission() == PermissionState::NotDetermined {
                    let _ = request_microphone_permission();
                } else {
                    open_system_privacy_settings("microphone")?;
                }
            }
            "accessibility" => {
                let st = prompt_accessibility_permission();
                if st != PermissionState::Granted {
                    open_system_privacy_settings("accessibility")?;
                }
            }
            other => {
                open_system_privacy_settings(other)?;
            }
        }
        self.emit_snapshot(app);
        Ok(())
    }
}

async fn fetch_remote_update_manifest(url: &str) -> Result<Option<UpdateManifest>> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .user_agent(format!("ttobak/{}", env!("CARGO_PKG_VERSION")))
        .build()?;
    let resp = client
        .get(url)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await?;

    let status = resp.status();
    if status == reqwest::StatusCode::NOT_FOUND && url.contains("api.github.com/repos/") {
        return Ok(None);
    }
    if status == reqwest::StatusCode::FORBIDDEN || status == reqwest::StatusCode::TOO_MANY_REQUESTS
    {
        return Err(anyhow!(
            "GitHub API 요청 한도를 초과했습니다. 잠시 후 다시 시도해주세요."
        ));
    }

    let resp = resp.error_for_status()?;
    let body = resp.text().await?;
    let manifest = parse_update_manifest_payload(&body, std::env::consts::ARCH)?;
    Ok(Some(manifest))
}

fn is_direct_installer_url(url: &str) -> bool {
    let path = url.split(['?', '#']).next().unwrap_or(url);
    let lower = path.to_ascii_lowercase();
    lower.ends_with(".dmg")
        || lower.ends_with(".pkg")
        || lower.ends_with(".zip")
        || lower.ends_with(".app.tar.gz")
}

async fn download_and_open_installer(url: &str, version: &str) -> Result<()> {
    let file_name = url
        .split(['?', '#'])
        .next()
        .and_then(|path| path.rsplit('/').next())
        .filter(|name| !name.is_empty())
        .map(|name| name.to_string())
        .unwrap_or_else(|| format!("ttobak-{}.dmg", version));

    let download_dir = dirs::download_dir().unwrap_or_else(std::env::temp_dir);
    tokio::fs::create_dir_all(&download_dir).await?;
    let dest_path = download_dir.join(&file_name);

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(180))
        .user_agent(format!("ttobak/{}", env!("CARGO_PKG_VERSION")))
        .build()?;
    let bytes = client
        .get(url)
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?;

    tokio::fs::write(&dest_path, &bytes).await?;

    Command::new("open")
        .arg(&dest_path)
        .spawn()
        .map_err(|e| anyhow!("다운로드한 설치 파일 열기 실패: {}", e))?;

    Ok(())
}

pub fn show_settings_window(app: &AppHandle) {
    if let Some(menu) = app.get_webview_window("menu") {
        let _ = menu.hide();
    }
    activate_macos_app();
    if let Some(win) = app.get_webview_window("settings") {
        let _ = win.show();
        let _ = win.unminimize();
        let _ = win.set_focus();
    }
}

fn position_and_style_hud(app: &AppHandle, ignores_mouse: bool) {
    let Some(hud) = app.get_webview_window("hud") else {
        return;
    };

    let focus_point = focused_target_screen_point();
    let _ = hud.set_ignore_cursor_events(ignores_mouse);

    let hud_for_main = hud.clone();
    let _ = app.run_on_main_thread(move || {
        if let Some((x, y)) = hud_origin_for_active_monitor(focus_point, 430.0, 200.0, 96.0) {
            let _ = hud_for_main.set_position(LogicalPosition::new(x, y));
        } else if let Ok(Some(monitor)) = hud_for_main.primary_monitor() {
            let m_size = monitor.size();
            let m_pos = monitor.position();
            let scale = monitor.scale_factor();
            let win_w = (430.0 * scale) as i32;
            let win_h = (200.0 * scale) as i32;
            let x = m_pos.x + (m_size.width as i32 - win_w) / 2;
            let y = m_pos.y + (m_size.height as i32) - win_h - (96.0 * scale) as i32;
            let _ = hud_for_main.set_position(PhysicalPosition::new(x, y));
        }

        if let Ok(ns_win) = hud_for_main.ns_window() {
            unsafe { configure_floating_hud_window(ns_win, ignores_mouse) };
        }
    });
}
