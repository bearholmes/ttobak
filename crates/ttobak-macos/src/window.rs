use objc2::msg_send;
use objc2::runtime::{AnyClass, AnyObject};
use objc2_app_kit::NSWorkspace;
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
