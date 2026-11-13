use anyhow::{Context, Result};
use core_foundation::base::TCFType;
use core_foundation::boolean::CFBoolean;
use core_foundation::dictionary::CFDictionary;
use core_foundation::string::CFString;
use objc2::rc::Retained;
use objc2::runtime::AnyClass;
use objc2::{msg_send, ClassType};
use objc2_foundation::NSString;
use std::ffi::c_void;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use ttobak_core::{PermissionSnapshot, PermissionState};

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrusted() -> bool;
    fn AXIsProcessTrustedWithOptions(options: *const c_void) -> bool;
}

#[link(name = "AVFoundation", kind = "framework")]
extern "C" {}

const LAUNCH_AGENT_LABEL: &str = "com.ttobak.app";

/// 현재 마이크 및 손쉬운 사용(현재 위치에 입력) 권한 상태를 조회합니다.
pub fn check_permissions() -> PermissionSnapshot {
    PermissionSnapshot {
        microphone: check_microphone_permission(),
        accessibility: check_accessibility_permission(),
    }
}

pub fn check_accessibility_permission() -> PermissionState {
    let trusted = unsafe { AXIsProcessTrusted() };
    if trusted {
        PermissionState::Granted
    } else {
        PermissionState::Denied
    }
}

/// 손쉬운 사용 시스템 권한 팝업을 띄우고 현재 신뢰 여부를 반환합니다.
pub fn prompt_accessibility_permission() -> PermissionState {
    let key = CFString::new("AXTrustedCheckOptionPrompt");
    let val = CFBoolean::true_value();
    let dict = CFDictionary::from_CFType_pairs(&[(key.as_CFType(), val.as_CFType())]);
    let trusted = unsafe { AXIsProcessTrustedWithOptions(dict.as_concrete_TypeRef() as *const _) };
    if trusted {
        PermissionState::Granted
    } else {
        PermissionState::Denied
    }
}

pub fn check_microphone_permission() -> PermissionState {
    unsafe {
        let Some(cls) = AnyClass::get(c"AVCaptureDevice") else {
            return PermissionState::NotDetermined;
        };
        let media_type: Retained<NSString> = NSString::from_str("soun");
        // AVAuthorizationStatus: 0 = NotDetermined, 1 = Restricted, 2 = Denied, 3 = Authorized
        let status: isize = msg_send![cls, authorizationStatusForMediaType: &*media_type];
        match status {
            3 => PermissionState::Granted,
            1 | 2 => PermissionState::Denied,
            _ => PermissionState::NotDetermined,
        }
    }
}

/// 마이크 권한이 아직 결정되지 않았을 때 macOS 기본 권한 요청 대화상자를 호출합니다.
pub fn request_microphone_permission() -> PermissionState {
    let current = check_microphone_permission();
    if current != PermissionState::NotDetermined {
        return current;
    }

    unsafe {
        let Some(cls) = AnyClass::get(c"AVCaptureDevice") else {
            return PermissionState::NotDetermined;
        };
        let media_type: Retained<NSString> = NSString::from_str("soun");
        let (tx, rx) = std::sync::mpsc::channel::<bool>();
        let block = block2::RcBlock::new(move |granted: objc2::runtime::Bool| {
            let _ = tx.send(granted.as_bool());
        });
        let _: () =
            msg_send![cls, requestAccessForMediaType: &*media_type, completionHandler: &*block];
        match rx.recv_timeout(std::time::Duration::from_secs(30)) {
            Ok(true) => PermissionState::Granted,
            Ok(false) => PermissionState::Denied,
            Err(_) => check_microphone_permission(),
        }
    }
}

/// macOS 시스템 설정의 해당 개인정보 보호 패널을 엽니다.
pub fn open_system_privacy_settings(target: &str) -> Result<()> {
    let url = match target {
        "microphone" => {
            "x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone"
        }
        "accessibility" => {
            "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility"
        }
        _ => "x-apple.systempreferences:com.apple.preference.security",
    };

    Command::new("open")
        .arg(url)
        .spawn()
        .context("시스템 설정 열기 실패")?;
    Ok(())
}

fn launch_agent_plist_path() -> Option<PathBuf> {
    dirs::home_dir().map(|h| {
        h.join("Library")
            .join("LaunchAgents")
            .join(format!("{}.plist", LAUNCH_AGENT_LABEL))
    })
}

pub fn is_launch_at_login_enabled() -> bool {
    launch_agent_plist_path()
        .map(|p| p.exists())
        .unwrap_or(false)
}

/// 로그인 시 자동 시작 LaunchAgent 등록/해제
pub fn set_launch_at_login(enabled: bool) -> Result<()> {
    let plist_path = launch_agent_plist_path().context("홈 디렉터리를 찾을 수 없습니다")?;
    if !enabled {
        if plist_path.exists() {
            fs::remove_file(&plist_path)?;
        }
        return Ok(());
    }

    if let Some(parent) = plist_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let exe = std::env::current_exe().context("현재 실행 파일 경로를 확인할 수 없습니다")?;
    let exe_str = exe.to_string_lossy();

    let plist_xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{label}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{exe}</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <false/>
</dict>
</plist>
"#,
        label = LAUNCH_AGENT_LABEL,
        exe = xml_escape(&exe_str),
    );

    fs::write(&plist_path, plist_xml)?;
    let _ = NSString::class(); // 링크 유지용
    Ok(())
}

fn xml_escape(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
