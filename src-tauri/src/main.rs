#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod coordinator;

use coordinator::{show_settings_window, SessionCoordinator};
use parking_lot::Mutex;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{
    image::Image,
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    ActivationPolicy, AppHandle, Manager, PhysicalPosition, State, WindowEvent,
};
use ttobak_core::{
    AppSnapshot, KeywordUsage, ModelDeleteTarget, ModelVariant, ShortcutConfig,
};
use ttobak_macos::window::present_menu_popover_window;

#[derive(Default)]
struct MenuPopoverGuard {
    opened_at: Mutex<Option<Instant>>,
    blurred_at: Mutex<Option<Instant>>,
}

#[tauri::command]
fn get_snapshot(coord: State<'_, Arc<SessionCoordinator>>) -> AppSnapshot {
    coord.build_snapshot()
}

#[tauri::command]
async fn toggle_recording(
    app: AppHandle,
    coord: State<'_, Arc<SessionCoordinator>>,
) -> Result<(), String> {
    let c = Arc::clone(&coord);
    c.toggle_recording(&app).await;
    Ok(())
}

#[tauri::command]
async fn cancel_session(
    app: AppHandle,
    coord: State<'_, Arc<SessionCoordinator>>,
) -> Result<(), String> {
    let c = Arc::clone(&coord);
    c.cancel_session(&app).await;
    Ok(())
}

#[tauri::command]
fn accept_first_notice(
    app: AppHandle,
    coord: State<'_, Arc<SessionCoordinator>>,
) -> Result<(), String> {
    coord
        .accept_first_notice(&app)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn update_shortcut(
    app: AppHandle,
    coord: State<'_, Arc<SessionCoordinator>>,
    shortcut: ShortcutConfig,
) -> Result<(), String> {
    coord
        .update_shortcut(&app, shortcut)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn set_shortcut_recording_paused(
    coord: State<'_, Arc<SessionCoordinator>>,
    paused: bool,
) -> Result<(), String> {
    coord.set_shortcut_recording_paused(paused);
    Ok(())
}

#[tauri::command]
fn update_keywords(
    app: AppHandle,
    coord: State<'_, Arc<SessionCoordinator>>,
    keywords_raw: String,
) -> Result<KeywordUsage, String> {
    coord
        .update_keywords(&app, keywords_raw)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn preview_keyword_usage(
    coord: State<'_, Arc<SessionCoordinator>>,
    keywords_raw: String,
) -> KeywordUsage {
    coord.preview_keyword_usage(&keywords_raw)
}

#[tauri::command]
fn set_launch_at_login(
    app: AppHandle,
    coord: State<'_, Arc<SessionCoordinator>>,
    enabled: bool,
) -> Result<(), String> {
    coord
        .update_launch_at_login(&app, enabled)
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn update_model_variant(
    app: AppHandle,
    coord: State<'_, Arc<SessionCoordinator>>,
    variant: ModelVariant,
) -> Result<(), String> {
    let c = Arc::clone(&coord);
    c.update_model_variant(&app, variant)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn download_models(
    app: AppHandle,
    coord: State<'_, Arc<SessionCoordinator>>,
) -> Result<(), String> {
    let c = Arc::clone(&coord);
    tauri::async_runtime::spawn(async move {
        c.start_model_download(&app).await;
    });
    Ok(())
}

#[tauri::command]
fn delete_models(
    app: AppHandle,
    coord: State<'_, Arc<SessionCoordinator>>,
    target: ModelDeleteTarget,
) -> Result<(), String> {
    coord
        .delete_models(&app, target)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn reveal_models_folder(coord: State<'_, Arc<SessionCoordinator>>) -> Result<(), String> {
    coord.reveal_models_folder().map_err(|e| e.to_string())
}

#[tauri::command]
fn open_external_url(url: String) -> Result<(), String> {
    let trimmed = url.trim();
    if !trimmed.starts_with("https://") {
        return Err("보안을 위해 https:// 주소만 열 수 있습니다.".to_string());
    }
    std::process::Command::new("open")
        .arg(trimmed)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("브라우저 열기 실패: {}", e))
}

#[tauri::command]
async fn check_app_update(
    app: AppHandle,
    coord: State<'_, Arc<SessionCoordinator>>,
) -> Result<(), String> {
    let c = Arc::clone(&coord);
    tauri::async_runtime::spawn(async move {
        c.check_for_app_update(&app).await;
    });
    Ok(())
}

#[tauri::command]
async fn apply_app_update(
    app: AppHandle,
    coord: State<'_, Arc<SessionCoordinator>>,
) -> Result<(), String> {
    let c = Arc::clone(&coord);
    c.apply_app_update(&app).await.map_err(|e| e.to_string())
}

#[tauri::command]
fn request_or_open_permission(
    app: AppHandle,
    coord: State<'_, Arc<SessionCoordinator>>,
    kind: String,
) -> Result<(), String> {
    coord
        .request_or_open_permission(&app, &kind)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn refresh_permissions(app: AppHandle, coord: State<'_, Arc<SessionCoordinator>>) -> AppSnapshot {
    coord.emit_snapshot(&app);
    coord.build_snapshot()
}

#[tauri::command]
fn copy_recovery_transcript(
    app: AppHandle,
    coord: State<'_, Arc<SessionCoordinator>>,
) -> Result<(), String> {
    coord
        .copy_recovery_and_dismiss(&app)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn dismiss_recovery(app: AppHandle, coord: State<'_, Arc<SessionCoordinator>>) {
    coord.dismiss_recovery(&app);
}

#[tauri::command]
fn open_settings(app: AppHandle) {
    show_settings_window(&app);
}

#[tauri::command]
fn hide_menu_popover(app: AppHandle) {
    if let Some(menu) = app.get_webview_window("menu") {
        let _ = menu.hide();
    }
}

#[tauri::command]
fn quit_app(app: AppHandle) {
    app.exit(0);
}

fn toggle_menu_popover_at_tray(app: &AppHandle, tray_x: f64, tray_y: f64, tray_w: f64, tray_h: f64) {
    let Some(menu_win) = app.get_webview_window("menu") else {
        return;
    };

    let guard = app.state::<Arc<MenuPopoverGuard>>();

    if menu_win.is_visible().unwrap_or(false) {
        let _ = menu_win.hide();
        return;
    }

    // 트레이 아이콘 마우스 다운 시점에 Focused(false)로 방금 닫힌 경우 재오픈 방지
    if let Some(blurred) = guard.blurred_at.lock().take() {
        if blurred.elapsed() < Duration::from_millis(260) {
            return;
        }
    }

    // 팝오버를 열기 직전 작업 중이던 외부 앱 PID 저장 및 최신 권한 상태 즉시 반영
    let coord = app.state::<Arc<SessionCoordinator>>();
    coord.remember_frontmost_external_app();
    coord.emit_snapshot(app);

    let scale = menu_win.scale_factor().unwrap_or(2.0);
    let win_w = 280.0 * scale;
    let target_x = (tray_x + tray_w / 2.0 - win_w / 2.0).max(12.0) as i32;
    let target_y = (tray_y + tray_h) as i32;

    *guard.opened_at.lock() = Some(Instant::now());
    let _ = menu_win.set_position(PhysicalPosition::new(target_x, target_y));

    let menu_for_main = menu_win.clone();
    let _ = app.run_on_main_thread(move || {
        if let Ok(ns_win) = menu_for_main.ns_window() {
            unsafe { present_menu_popover_window(ns_win) };
        }
        let _ = menu_for_main.show();
        let _ = menu_for_main.set_focus();
    });
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,ttobak=debug".into()),
        )
        .init();

    let coordinator = SessionCoordinator::initialize().expect("또박 초기화 실패");
    let popover_guard = Arc::new(MenuPopoverGuard::default());

    tauri::Builder::default()
        .manage(Arc::clone(&coordinator))
        .manage(Arc::clone(&popover_guard))
        .setup(move |app| {
            // Dock 아이콘 없이 메뉴 막대 전용 앱으로 동작 (LSUIElement)
            app.set_activation_policy(ActivationPolicy::Accessory);

            let tray_png = include_bytes!("../icons/tray-icon.png");
            let tray_img = Image::from_bytes(tray_png).expect("트레이 아이콘 로드 실패");

            // 우클릭 보조 네이티브 메뉴 (비상시에도 즉시 말하기 완료·취소 가능)
            let toggle_item =
                MenuItem::with_id(app, "toggle_voice", "말하기 시작 / 완료", true, None::<&str>)?;
            let cancel_item =
                MenuItem::with_id(app, "cancel_voice", "진행 중인 입력 취소 (Esc)", true, None::<&str>)?;
            let sep1 = PredefinedMenuItem::separator(app)?;
            let settings_item = MenuItem::with_id(app, "settings", "설정...", true, None::<&str>)?;
            let update_item =
                MenuItem::with_id(app, "check_update", "업데이트 확인...", true, None::<&str>)?;
            let sep2 = PredefinedMenuItem::separator(app)?;
            let quit_item = MenuItem::with_id(app, "quit", "종료", true, None::<&str>)?;
            let fallback_menu = Menu::with_items(
                app,
                &[
                    &toggle_item,
                    &cancel_item,
                    &sep1,
                    &settings_item,
                    &update_item,
                    &sep2,
                    &quit_item,
                ],
            )?;

            let _tray = TrayIconBuilder::with_id("ttobak-tray")
                .icon(tray_img)
                .icon_as_template(true)
                .tooltip("또박 — 말하면, 글이 돼요")
                .menu(&fallback_menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app_handle, event| match event.id().as_ref() {
                    "toggle_voice" => {
                        let c = Arc::clone(&app_handle.state::<Arc<SessionCoordinator>>());
                        let a = app_handle.clone();
                        tauri::async_runtime::spawn(async move {
                            c.toggle_recording(&a).await;
                        });
                    }
                    "cancel_voice" => {
                        let c = Arc::clone(&app_handle.state::<Arc<SessionCoordinator>>());
                        let a = app_handle.clone();
                        tauri::async_runtime::spawn(async move {
                            c.cancel_session(&a).await;
                        });
                    }
                    "settings" => show_settings_window(app_handle),
                    "check_update" => {
                        show_settings_window(app_handle);
                        let c = Arc::clone(&app_handle.state::<Arc<SessionCoordinator>>());
                        let a = app_handle.clone();
                        tauri::async_runtime::spawn(async move {
                            c.check_for_app_update(&a).await;
                        });
                    }
                    "quit" => app_handle.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        rect,
                        ..
                    } = event
                    {
                        let pos = rect.position.to_physical::<f64>(1.0);
                        let sz = rect.size.to_physical::<f64>(1.0);
                        toggle_menu_popover_at_tray(
                            tray.app_handle(),
                            pos.x,
                            pos.y,
                            sz.width,
                            sz.height,
                        );
                    }
                })
                .build(app)?;

            coordinator.bind_runtime(app.handle().clone());
            Ok(())
        })
        .on_window_event(|window, event| match window.label() {
            // 메뉴 막대 팝오버 외부 클릭 시 자동 숨김 (오픈 직후 초기 포커스 전환 노이즈 무시)
            "menu" => match event {
                WindowEvent::Focused(true) => {
                    let coord = window.app_handle().state::<Arc<SessionCoordinator>>();
                    coord.emit_snapshot(window.app_handle());
                }
                WindowEvent::Focused(false) => {
                    let guard = window.app_handle().state::<Arc<MenuPopoverGuard>>();
                    let just_opened = guard
                        .opened_at
                        .lock()
                        .map(|t| t.elapsed() < Duration::from_millis(220))
                        .unwrap_or(false);
                    if !just_opened && window.is_visible().unwrap_or(false) {
                        *guard.blurred_at.lock() = Some(Instant::now());
                        let _ = window.hide();
                    }
                }
                _ => {}
            },
            // 설정 창 포커스 복귀 시 권한 상태 즉시 새로고침 / 닫기(X) 시 숨김 처리
            "settings" => match event {
                WindowEvent::Focused(true) => {
                    let coord = window.app_handle().state::<Arc<SessionCoordinator>>();
                    coord.emit_snapshot(window.app_handle());
                }
                WindowEvent::CloseRequested { api, .. } => {
                    api.prevent_close();
                    let _ = window.hide();
                }
                _ => {}
            },
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            get_snapshot,
            toggle_recording,
            cancel_session,
            accept_first_notice,
            update_shortcut,
            set_shortcut_recording_paused,
            update_keywords,
            preview_keyword_usage,
            set_launch_at_login,
            update_model_variant,
            download_models,
            delete_models,
            reveal_models_folder,
            open_external_url,
            check_app_update,
            apply_app_update,
            request_or_open_permission,
            refresh_permissions,
            copy_recovery_transcript,
            dismiss_recovery,
            open_settings,
            hide_menu_popover,
            quit_app,
        ])
        .run(tauri::generate_context!())
        .expect("또박 실행 중 오류가 발생했습니다");
}
