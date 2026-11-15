use objc2::msg_send;
use objc2::runtime::{AnyClass, AnyObject};
use objc2_app_kit::{NSEvent, NSScreen, NSWorkspace};
use objc2_foundation::MainThreadMarker;
use std::ffi::c_void;
use std::ptr;

unsafe fn clear_window_chrome_and_shadow(win: &AnyObject) {
    let _: () = msg_send![win, setOpaque: false];
    let _: () = msg_send![win, setHasShadow: false];
    if let Some(color_cls) = AnyClass::get(c"NSColor") {
        let clear: *mut AnyObject = msg_send![color_cls, clearColor];
        if !clear.is_null() {
            let _: () = msg_send![win, setBackgroundColor: clear];
        }
    }
    let _: () = msg_send![win, invalidateShadow];
}

/// 플로팅 HUD 윈도우(`NSWindow`)가 전체 화면 앱이나 다른 데스크톱 스페이스 위에서도
/// 포커스를 뺏지 않고 항상 최상단에 부드럽게 떠 있도록 네이티브 속성을 설정합니다.
///
/// # Safety
/// `ns_window_ptr`는 유효한 `NSWindow *` 포인터여야 하며 메인 스레드에서 호출되어야 합니다.
pub unsafe fn configure_floating_hud_window(ns_window_ptr: *mut c_void, ignores_mouse: bool) {
    if ns_window_ptr.is_null() {
        return;
    }
    let win = &*(ns_window_ptr as *const AnyObject);

    clear_window_chrome_and_shadow(win);

    // NSStatusWindowLevel (25) — 일반 창 위에 표시하되 메뉴 팝오버(101)보다 아래에 유지
    let level: isize = 25;
    let _: () = msg_send![win, setLevel: level];

    // CanJoinAllSpaces (1 << 0) | Stationary (1 << 4) | IgnoresCycle (1 << 6) | FullScreenAuxiliary (1 << 8)
    let behavior: usize = (1 << 0) | (1 << 4) | (1 << 6) | (1 << 8);
    let _: () = msg_send![win, setCollectionBehavior: behavior];

    let _: () = msg_send![win, setHidesOnDeactivate: false];
    let _: () = msg_send![win, setIgnoresMouseEvents: ignores_mouse];
}

/// 포커스를 전혀 빼앗지 않고(`makeKeyAndOrderFront` 호출 없이) 플로팅 HUD 창만 화면에 표시합니다.
///
/// # Safety
/// `ns_window_ptr`는 유효한 `NSWindow *` 포인터여야 하며 메인 스레드에서 호출되어야 합니다.
pub unsafe fn show_hud_without_activating(ns_window_ptr: *mut c_void) {
    if ns_window_ptr.is_null() {
        return;
    }
    let win = &*(ns_window_ptr as *const AnyObject);
    let _: () = msg_send![win, orderFrontRegardless];
}

/// 포커스 전환 부작용 없이 플로팅 HUD 창을 숨깁니다.
///
/// # Safety
/// `ns_window_ptr`는 유효한 `NSWindow *` 포인터여야 하며 메인 스레드에서 호출되어야 합니다.
pub unsafe fn hide_hud_without_activating(ns_window_ptr: *mut c_void) {
    if ns_window_ptr.is_null() {
        return;
    }
    let win = &*(ns_window_ptr as *const AnyObject);
    let null_sender: *const AnyObject = ptr::null();
    let _: () = msg_send![win, orderOut: null_sender];
}

/// 트레이 메뉴 팝오버 창을 팝업 메뉴 레벨(101)로 올리고 앱 활성화와 함께 키 윈도우로 표시합니다.
///
/// # Safety
/// `ns_window_ptr`는 유효한 `NSWindow *` 포인터여야 하며 메인 스레드에서 호출되어야 합니다.
pub unsafe fn present_menu_popover_window(ns_window_ptr: *mut c_void) {
    if ns_window_ptr.is_null() {
        return;
    }
    if let Some(app_cls) = AnyClass::get(c"NSApplication") {
        let ns_app: *mut AnyObject = msg_send![app_cls, sharedApplication];
        if !ns_app.is_null() {
            let _: () = msg_send![ns_app, activateIgnoringOtherApps: true];
        }
    }

    let win = &*(ns_window_ptr as *const AnyObject);
    clear_window_chrome_and_shadow(win);
    // NSPopUpMenuWindowLevel (101) — HUD(25)보다 항상 위
    let level: isize = 101;
    let _: () = msg_send![win, setLevel: level];
    let behavior: usize = (1 << 0) | (1 << 4) | (1 << 6) | (1 << 8);
    let _: () = msg_send![win, setCollectionBehavior: behavior];
    let null_sender: *const AnyObject = ptr::null();
    let _: () = msg_send![win, makeKeyAndOrderFront: null_sender];
}

/// 설정 창 등 일반 창을 표시할 때 Accessory 앱(`LSUIElement`)을 전면 활성화합니다.
pub fn activate_macos_app() {
    unsafe {
        if let Some(app_cls) = AnyClass::get(c"NSApplication") {
            let ns_app: *mut AnyObject = msg_send![app_cls, sharedApplication];
            if !ns_app.is_null() {
                let _: () = msg_send![ns_app, activateIgnoringOtherApps: true];
            }
        }
    }
}

/// 현재 전면 앱이 `또박` 자신이 아니면 해당 프로세스 PID를 반환합니다.
pub fn current_external_frontmost_pid() -> Option<i32> {
    let workspace = NSWorkspace::sharedWorkspace();
    let app = workspace.frontmostApplication()?;
    let bid = app.bundleIdentifier().map(|s| s.to_string());
    if bid.as_deref() == Some("com.ttobak.app") {
        return None;
    }
    let pid = app.processIdentifier();
    if pid > 0 {
        Some(pid)
    } else {
        None
    }
}

/// 트레이 팝오버를 통해 녹음을 종료했을 때 직전 작업 중이던 외부 앱으로 포커스를 복원합니다.
pub fn restore_external_app_focus_if_needed(saved_pid: Option<i32>) -> bool {
    let Some(pid) = saved_pid else {
        return false;
    };
    let workspace = NSWorkspace::sharedWorkspace();
    let is_self_frontmost = workspace
        .frontmostApplication()
        .and_then(|a| a.bundleIdentifier().map(|s| s.to_string()))
        .as_deref()
        == Some("com.ttobak.app");

    if !is_self_frontmost {
        return false;
    }

    unsafe {
        let Some(cls) = AnyClass::get(c"NSRunningApplication") else {
            return false;
        };
        let running_app: *mut AnyObject =
            msg_send![cls, runningApplicationWithProcessIdentifier: pid];
        if running_app.is_null() {
            return false;
        }
        // NSApplicationActivateIgnoringOtherApps = 1 << 1 = 2
        let options: usize = 2;
        let ok: bool = msg_send![running_app, activateWithOptions: options];
        ok
    }
}

/// Cocoa 좌표계(`(x, y, width, height)`, 주 모니터 좌하단 원점) 사각형을 기준으로
/// 포커스 좌표(Quartz 좌상단 원점) 또는 마우스 좌표(Cocoa 좌하단 원점)가 속한 모니터를 선택해
/// HUD 창의 Quartz 좌상단 원점 논리 좌표를 계산합니다.
pub fn compute_hud_origin_quartz(
    screens_cocoa: &[(f64, f64, f64, f64)],
    focus_point_quartz: Option<(f64, f64)>,
    mouse_point_cocoa: Option<(f64, f64)>,
    hud_width: f64,
    hud_height: f64,
    bottom_margin: f64,
) -> Option<(f64, f64)> {
    let primary = *screens_cocoa.first()?;
    let primary_height = primary.3;

    let contains_cocoa = |rect: (f64, f64, f64, f64), pt: (f64, f64)| -> bool {
        pt.0 >= rect.0 && pt.0 < rect.0 + rect.2 && pt.1 >= rect.1 && pt.1 < rect.1 + rect.3
    };

    let selected = focus_point_quartz
        .and_then(|(qx, qy)| {
            let cocoa_pt = (qx, primary_height - qy);
            screens_cocoa
                .iter()
                .copied()
                .find(|&rect| contains_cocoa(rect, cocoa_pt))
        })
        .or_else(|| {
            mouse_point_cocoa.and_then(|cocoa_pt| {
                screens_cocoa
                    .iter()
                    .copied()
                    .find(|&rect| contains_cocoa(rect, cocoa_pt))
            })
        })
        .unwrap_or(primary);

    let quartz_left = selected.0;
    let quartz_top = primary_height - (selected.1 + selected.3);
    let x = quartz_left + (selected.2 - hud_width) / 2.0;
    let y = quartz_top + selected.3 - hud_height - bottom_margin;
    Some((x, y))
}

/// 현재 포커스된 입력창(1순위) 또는 마우스 커서(2순위)가 위치한 모니터의 하단 중앙에
/// HUD 창을 배치할 논리 좌표(`LogicalPosition`)를 반환합니다.
///
/// 메인 스레드에서 호출되어야 합니다.
pub fn hud_origin_for_active_monitor(
    focus_point_quartz: Option<(f64, f64)>,
    hud_width: f64,
    hud_height: f64,
    bottom_margin: f64,
) -> Option<(f64, f64)> {
    let mtm = MainThreadMarker::new()?;
    let screens = NSScreen::screens(mtm);
    let rects: Vec<(f64, f64, f64, f64)> = screens
        .iter()
        .map(|screen| {
            let f = screen.frame();
            (f.origin.x, f.origin.y, f.size.width, f.size.height)
        })
        .collect();

    let mouse = NSEvent::mouseLocation();
    compute_hud_origin_quartz(
        &rects,
        focus_point_quartz,
        Some((mouse.x, mouse.y)),
        hud_width,
        hud_height,
        bottom_margin,
    )
}

#[cfg(test)]
mod tests {
    use super::compute_hud_origin_quartz;

    #[test]
    fn test_compute_hud_origin_selects_secondary_monitor_by_focus_or_mouse() {
        // 메인 모니터: 1512x982 @ (0, 0)
        // 오른쪽 보조 모니터: 2560x1440 @ (1512, -229) (Cocoa 좌표)
        let screens = vec![(0.0, 0.0, 1512.0, 982.0), (1512.0, -229.0, 2560.0, 1440.0)];

        // 1) 포커스 좌표가 보조 모니터(Quartz x=2000, y=300 -> Cocoa y=982-300=682)에 있을 때
        let pos_focus = compute_hud_origin_quartz(
            &screens,
            Some((2000.0, 300.0)),
            Some((200.0, 200.0)), // 마우스는 메인 모니터에 있어도 포커스 우선
            430.0,
            200.0,
            96.0,
        )
        .unwrap();
        // 보조 모니터 Quartz top = 982 - (-229 + 1440) = -229
        // x = 1512 + (2560 - 430)/2 = 2577
        // y = -229 + 1440 - 200 - 96 = 915
        assert_eq!(pos_focus, (2577.0, 915.0));

        // 2) 포커스 좌표가 없고 마우스 커서가 보조 모니터에 있을 때
        let pos_mouse = compute_hud_origin_quartz(
            &screens,
            None,
            Some((2100.0, 500.0)),
            430.0,
            200.0,
            96.0,
        )
        .unwrap();
        assert_eq!(pos_mouse, (2577.0, 915.0));

        // 3) 둘 다 메인 모니터 또는 범위 밖이면 메인 모니터로 폴백
        let pos_main =
            compute_hud_origin_quartz(&screens, None, None, 430.0, 200.0, 96.0).unwrap();
        assert_eq!(pos_main, ((1512.0 - 430.0) / 2.0, 982.0 - 200.0 - 96.0));
    }
}
