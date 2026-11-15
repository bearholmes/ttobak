use core_foundation::base::{CFRelease, CFTypeRef, TCFType};
use core_foundation::boolean::CFBoolean;
use core_foundation::string::{CFString, CFStringRef};
use objc2_app_kit::NSWorkspace;
use std::ffi::c_void;
use std::ptr;

type AXUIElementRef = *mut c_void;
type AXValueRef = *mut c_void;
type AXError = i32;
const K_AX_ERROR_SUCCESS: AXError = 0;
const K_AX_VALUE_CG_POINT_TYPE: u32 = 1;
const K_AX_VALUE_CG_SIZE_TYPE: u32 = 2;

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
struct AxCGPoint {
    x: f64,
    y: f64,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
struct AxCGSize {
    width: f64,
    height: f64,
}

#[link(name = "Carbon", kind = "framework")]
extern "C" {
    fn IsSecureEventInputEnabled() -> u8;
}

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrusted() -> bool;
    fn AXUIElementCreateSystemWide() -> AXUIElementRef;
    fn AXUIElementCreateApplication(pid: i32) -> AXUIElementRef;
    fn AXUIElementCopyAttributeValue(
        element: AXUIElementRef,
        attribute: CFStringRef,
        value: *mut CFTypeRef,
    ) -> AXError;
    fn AXUIElementSetAttributeValue(
        element: AXUIElementRef,
        attribute: CFStringRef,
        value: CFTypeRef,
    ) -> AXError;
    fn AXUIElementIsAttributeSettable(
        element: AXUIElementRef,
        attribute: CFStringRef,
        settable: *mut u8,
    ) -> AXError;
    fn AXValueGetValue(value: AXValueRef, value_type: u32, value_ptr: *mut c_void) -> bool;
}

/// AX 트리상 일반 텍스트 필드를 노출하지 않지만 항상 키 입력을 받는 터미널 앱 번들 ID 목록
const TERMINAL_BUNDLE_IDS: &[&str] = &[
    "com.apple.Terminal",
    "com.googlecode.iterm2",
    "com.mitchellh.ghostty",
    "dev.warp.Warp-Stable",
    "net.kovidgoyal.kitty",
    "com.github.wez.wezterm",
    "org.alacritty",
    "co.zeit.hyper",
    "org.tabby",
];

/// 클립보드 동기화 지연이 길어 복원 유예 시간(800ms)을 늘려야 하는 가상머신/원격 앱 번들 ID 접두사
const VM_BUNDLE_PREFIXES: &[&str] = &[
    "com.parallels.",
    "com.vmware.fusion",
    "com.utmapp.UTM",
    "com.microsoft.rdc",
];

/// 포커스 및 보안 입력 검사 결과
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FocusStatus {
    /// 비밀번호 입력란 등 보안 입력 활성 상태 (녹음 시작 차단 및 붙여넣기 차단)
    SecureInputBlocked { reason: String },
    /// 텍스트 붙여넣기(`Cmd+V`)가 가능한 유효 입력 대상
    EditableTarget {
        bundle_id: Option<String>,
        is_vm: bool,
    },
    /// 손쉬운 사용 권한이 없거나 포커스된 텍스트 입력창을 찾을 수 없음 (복구 카드 폴백 대상)
    NoEditableTarget { reason: String },
}

/// Carbon Secure Event Input 여부만 빠르게 확인합니다.
pub fn is_carbon_secure_input_enabled() -> bool {
    unsafe { IsSecureEventInputEnabled() != 0 }
}

/// 현재 전면 앱과 포커스된 UI 요소를 검사해 비밀번호 입력란 여부와 텍스트 입력 가능 여부를 판정합니다.
pub fn inspect_focused_target() -> FocusStatus {
    // 1차 방어선: macOS 전역 Secure Event Input 확인 (비밀번호 필드, 터미널 sudo 등)
    if is_carbon_secure_input_enabled() {
        return FocusStatus::SecureInputBlocked {
            reason: "비밀번호 입력란에서는 녹음이나 입력을 하지 않아요.".to_string(),
        };
    }

    let (front_pid, bundle_id) = frontmost_app_info();
    let is_vm = bundle_id
        .as_deref()
        .map(|b| VM_BUNDLE_PREFIXES.iter().any(|prefix| b.starts_with(prefix)))
        .unwrap_or(false);

    // 자체 앱(`com.ttobak.app`)이 전면에 활성화된 경우(예: 설정 창 테스트 입력칸 외 빈 영역)도 AX로 정상 검사
    let trusted = unsafe { AXIsProcessTrusted() };
    if !trusted {
        return FocusStatus::NoEditableTarget {
            reason: "‘현재 위치에 입력’(손쉬운 사용) 권한이 꺼져 있어 카드로 결과를 보여드려요."
                .to_string(),
        };
    }

    // Electron / Chromium 계열 앱이 내부 입력창 AX 트리를 활성화하도록 깨움
    if let Some(pid) = front_pid {
        wake_manual_accessibility(pid);
    }

    // 2차 방어선: Accessibility 포커스 요소의 AXSubrole / AXRole 검사
    let sys_wide = unsafe { AXUIElementCreateSystemWide() };
    if sys_wide.is_null() {
        return FocusStatus::NoEditableTarget {
            reason: "입력할 위치를 확인하지 못했어요.".to_string(),
        };
    }

    let focused_attr = CFString::new("AXFocusedUIElement");
    let mut focused_ref: CFTypeRef = ptr::null();
    let err = unsafe {
        AXUIElementCopyAttributeValue(
            sys_wide,
            focused_attr.as_concrete_TypeRef(),
            &mut focused_ref,
        )
    };
    unsafe { CFRelease(sys_wide as CFTypeRef) };

    if err == K_AX_ERROR_SUCCESS && !focused_ref.is_null() {
        let elem = focused_ref as AXUIElementRef;
        let subrole = copy_ax_string_attr(elem, "AXSubrole").unwrap_or_default();
        let role = copy_ax_string_attr(elem, "AXRole").unwrap_or_default();
        let role_desc = copy_ax_string_attr(elem, "AXRoleDescription").unwrap_or_default();

        if subrole == "AXSecureTextField"
            || subrole.to_ascii_lowercase().contains("secure")
            || role_desc.to_ascii_lowercase().contains("secure text")
            || role_desc.contains("보안 텍스트")
        {
            unsafe { CFRelease(focused_ref) };
            return FocusStatus::SecureInputBlocked {
                reason: "비밀번호 입력란에서는 녹음이나 입력을 하지 않아요.".to_string(),
            };
        }

        let editable = is_ax_element_editable(elem, &role);
        unsafe { CFRelease(focused_ref) };

        if editable {
            return FocusStatus::EditableTarget { bundle_id, is_vm };
        }
    }

    // 터미널 및 가상머신 창은 AX 텍스트 요소가 없어도 키보드 붙여넣기를 수용함
    if let Some(ref bid) = bundle_id {
        if TERMINAL_BUNDLE_IDS.iter().any(|&t| t.eq_ignore_ascii_case(bid)) || is_vm {
            return FocusStatus::EditableTarget { bundle_id, is_vm };
        }
    }

    FocusStatus::NoEditableTarget {
        reason: "커서가 놓인 입력창을 찾지 못해 카드로 결과를 보여드려요.".to_string(),
    }
}

fn frontmost_app_info() -> (Option<i32>, Option<String>) {
    let workspace = NSWorkspace::sharedWorkspace();
    let Some(app) = workspace.frontmostApplication() else {
        return (None, None);
    };
    let pid = app.processIdentifier();
    let bundle_id = app.bundleIdentifier().map(|s| s.to_string());
    (Some(pid), bundle_id)
}

fn wake_manual_accessibility(pid: i32) {
    unsafe {
        let app_elem = AXUIElementCreateApplication(pid);
        if app_elem.is_null() {
            return;
        }
        let attr = CFString::new("AXManualAccessibility");
        let _ = AXUIElementSetAttributeValue(
            app_elem,
            attr.as_concrete_TypeRef(),
            CFBoolean::true_value().as_CFTypeRef(),
        );
        CFRelease(app_elem as CFTypeRef);
    }
}

fn copy_ax_string_attr(elem: AXUIElementRef, attr_name: &str) -> Option<String> {
    let attr = CFString::new(attr_name);
    let mut val_ref: CFTypeRef = ptr::null();
    let err =
        unsafe { AXUIElementCopyAttributeValue(elem, attr.as_concrete_TypeRef(), &mut val_ref) };
    if err != K_AX_ERROR_SUCCESS || val_ref.is_null() {
        return None;
    }
    let cf_type_id = unsafe { core_foundation::base::CFGetTypeID(val_ref) };
    let res = if cf_type_id == CFString::type_id() {
        let cf_str = unsafe { CFString::wrap_under_create_rule(val_ref as CFStringRef) };
        Some(cf_str.to_string())
    } else {
        unsafe { CFRelease(val_ref) };
        None
    };
    res
}

fn copy_ax_bool_attr(elem: AXUIElementRef, attr_name: &str) -> Option<bool> {
    let attr = CFString::new(attr_name);
    let mut val_ref: CFTypeRef = ptr::null();
    let err =
        unsafe { AXUIElementCopyAttributeValue(elem, attr.as_concrete_TypeRef(), &mut val_ref) };
    if err != K_AX_ERROR_SUCCESS || val_ref.is_null() {
        return None;
    }
    let cf_type_id = unsafe { core_foundation::base::CFGetTypeID(val_ref) };
    if cf_type_id == CFBoolean::type_id() {
        let b = unsafe { CFBoolean::wrap_under_create_rule(val_ref as _) };
        Some(bool::from(b))
    } else {
        unsafe { CFRelease(val_ref) };
        None
    }
}

fn is_ax_element_editable(elem: AXUIElementRef, role: &str) -> bool {
    if matches!(
        role,
        "AXTextField" | "AXTextArea" | "AXComboBox" | "AXSearchField"
    ) {
        return true;
    }

    if copy_ax_bool_attr(elem, "AXEditable") == Some(true) {
        return true;
    }

    // 웹 에디터(Notion, Google Docs, Monaco, CodeMirror, Slack 등)는 AXValue가 settable이거나 AXSelectedTextRange가 존재함
    let value_attr = CFString::new("AXValue");
    let mut settable: u8 = 0;
    let err = unsafe {
        AXUIElementIsAttributeSettable(elem, value_attr.as_concrete_TypeRef(), &mut settable)
    };
    if err == K_AX_ERROR_SUCCESS && settable != 0 && role != "AXWindow" && role != "AXApplication"
    {
        return true;
    }

    let sel_attr = CFString::new("AXSelectedTextRange");
    let mut sel_val: CFTypeRef = ptr::null();
    let sel_err =
        unsafe { AXUIElementCopyAttributeValue(elem, sel_attr.as_concrete_TypeRef(), &mut sel_val) };
    if sel_err == K_AX_ERROR_SUCCESS && !sel_val.is_null() {
        unsafe { CFRelease(sel_val) };
        if matches!(role, "AXWebArea" | "AXGroup" | "AXStaticText" | "AXDocument") {
            return true;
        }
    }

    false
}

/// 현재 키보드 포커스가 놓인 UI 요소 또는 포커스된 윈도우의 중심 좌표(Quartz 전역 논리 좌표)를 반환합니다.
pub fn focused_target_screen_point() -> Option<(f64, f64)> {
    if !unsafe { AXIsProcessTrusted() } {
        return None;
    }

    let sys_wide = unsafe { AXUIElementCreateSystemWide() };
    if !sys_wide.is_null() {
        let focused_attr = CFString::new("AXFocusedUIElement");
        let mut focused_ref: CFTypeRef = ptr::null();
        let err = unsafe {
            AXUIElementCopyAttributeValue(
                sys_wide,
                focused_attr.as_concrete_TypeRef(),
                &mut focused_ref,
            )
        };
        unsafe { CFRelease(sys_wide as CFTypeRef) };

        if err == K_AX_ERROR_SUCCESS && !focused_ref.is_null() {
            let center = ax_element_center_point(focused_ref as AXUIElementRef);
            unsafe { CFRelease(focused_ref) };
            if center.is_some() {
                return center;
            }
        }
    }

    let (front_pid, _) = frontmost_app_info();
    let pid = front_pid?;
    let app_elem = unsafe { AXUIElementCreateApplication(pid) };
    if app_elem.is_null() {
        return None;
    }

    let win_attr = CFString::new("AXFocusedWindow");
    let mut win_ref: CFTypeRef = ptr::null();
    let err = unsafe {
        AXUIElementCopyAttributeValue(app_elem, win_attr.as_concrete_TypeRef(), &mut win_ref)
    };
    unsafe { CFRelease(app_elem as CFTypeRef) };

    if err == K_AX_ERROR_SUCCESS && !win_ref.is_null() {
        let center = ax_element_center_point(win_ref as AXUIElementRef);
        unsafe { CFRelease(win_ref) };
        return center;
    }

    None
}

fn ax_element_center_point(elem: AXUIElementRef) -> Option<(f64, f64)> {
    let pos = copy_ax_point_attr(elem, "AXPosition")?;
    let size = copy_ax_size_attr(elem, "AXSize").unwrap_or_default();

    // (0, 0) 크기의 더미 요소는 무시하고 상위 윈도우 폴백으로 넘김
    if size.width <= 0.0 && size.height <= 0.0 && pos.x == 0.0 && pos.y == 0.0 {
        return None;
    }

    let cx = pos.x + (size.width.max(0.0) / 2.0);
    let cy = pos.y + (size.height.max(0.0) / 2.0);
    if cx.is_finite() && cy.is_finite() {
        Some((cx, cy))
    } else {
        None
    }
}

fn copy_ax_point_attr(elem: AXUIElementRef, attr_name: &str) -> Option<AxCGPoint> {
    let attr = CFString::new(attr_name);
    let mut val_ref: CFTypeRef = ptr::null();
    let err =
        unsafe { AXUIElementCopyAttributeValue(elem, attr.as_concrete_TypeRef(), &mut val_ref) };
    if err != K_AX_ERROR_SUCCESS || val_ref.is_null() {
        return None;
    }
    let mut pt = AxCGPoint::default();
    let ok = unsafe {
        AXValueGetValue(
            val_ref as AXValueRef,
            K_AX_VALUE_CG_POINT_TYPE,
            (&mut pt as *mut AxCGPoint).cast(),
        )
    };
    unsafe { CFRelease(val_ref) };
    if ok {
        Some(pt)
    } else {
        None
    }
}

fn copy_ax_size_attr(elem: AXUIElementRef, attr_name: &str) -> Option<AxCGSize> {
    let attr = CFString::new(attr_name);
    let mut val_ref: CFTypeRef = ptr::null();
    let err =
        unsafe { AXUIElementCopyAttributeValue(elem, attr.as_concrete_TypeRef(), &mut val_ref) };
    if err != K_AX_ERROR_SUCCESS || val_ref.is_null() {
        return None;
    }
    let mut sz = AxCGSize::default();
    let ok = unsafe {
        AXValueGetValue(
            val_ref as AXValueRef,
            K_AX_VALUE_CG_SIZE_TYPE,
            (&mut sz as *mut AxCGSize).cast(),
        )
    };
    unsafe { CFRelease(val_ref) };
    if ok {
        Some(sz)
    } else {
        None
    }
}
