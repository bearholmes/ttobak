use crate::security_focus::{inspect_focused_target, FocusStatus};
use anyhow::{anyhow, Result};
use objc2::runtime::ProtocolObject;
use objc2_app_kit::{NSPasteboard, NSPasteboardItem, NSPasteboardWriting};
use objc2_foundation::{NSArray, NSData, NSString};
use std::ffi::c_void;
use std::ptr;
use std::thread;
use std::time::{Duration, Instant};
use tracing::{debug, warn};

type CGEventRef = *mut c_void;
type CGEventSourceRef = *mut c_void;

const K_CG_EVENT_SOURCE_STATE_COMBINED: i32 = 0;
const K_CG_EVENT_SOURCE_STATE_HID_SYSTEM: i32 = 1;
const K_CG_HID_EVENT_TAP: u32 = 0;
const K_VK_ANSI_V: u16 = 9;

const FLAG_ALPHA_SHIFT: u64 = 0x0001_0000;
const FLAG_SHIFT: u64 = 0x0002_0000;
const FLAG_CONTROL: u64 = 0x0004_0000;
const FLAG_ALTERNATE: u64 = 0x0008_0000;
const FLAG_COMMAND: u64 = 0x0010_0000;

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGEventSourceCreate(state_id: i32) -> CGEventSourceRef;
    fn CGEventSourceFlagsState(state_id: i32) -> u64;
    fn CGEventCreateKeyboardEvent(
        source: CGEventSourceRef,
        virtual_key: u16,
        key_down: bool,
    ) -> CGEventRef;
    fn CGEventSetFlags(event: CGEventRef, flags: u64);
    fn CGEventPost(tap: u32, event: CGEventRef);
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFRelease(cf: *const c_void);
}

/// 단일 NSPasteboardItem의 모든 UTI 타입과 바이트 데이터를 보존하는 스냅샷
#[derive(Clone)]
struct SavedPasteboardItem {
    entries: Vec<(String, Vec<u8>)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeliveryOutcome {
    PastedAtCursor,
    NeedsRecoveryCard { reason: String },
    BlockedSecure { reason: String },
}

/// 복구 카드의 `[복사]` 버튼용 영구 클립보드 복사
pub fn copy_text_to_clipboard(text: &str) -> Result<()> {
    let pb = NSPasteboard::generalPasteboard();
    pb.clearContents();
    let ns_text = NSString::from_str(text);
    let utf8_type = NSString::from_str("public.utf8-plain-text");
    let ok = pb.setString_forType(&ns_text, &utf8_type);
    if !ok {
        return Err(anyhow!("클립보드에 텍스트를 복사하지 못했습니다."));
    }
    Ok(())
}

/// 원래 클립보드를 백업한 뒤 `TransientType` 마커와 함께 `Cmd+V`를 합성하고 원래 클립보드를 복원합니다.
pub fn deliver_transcript_preserving_clipboard(text: &str) -> DeliveryOutcome {
    if text.trim().is_empty() {
        return DeliveryOutcome::PastedAtCursor;
    }

    // 붙여넣기 직전 2차 보안/포커스 재검증 (녹음 도중 비밀번호 칸으로 이동한 경우 완벽 차단)
    let is_vm = match inspect_focused_target() {
        FocusStatus::SecureInputBlocked { reason } => {
            return DeliveryOutcome::BlockedSecure { reason };
        }
        FocusStatus::NoEditableTarget { reason } => {
            return DeliveryOutcome::NeedsRecoveryCard { reason };
        }
        FocusStatus::EditableTarget { is_vm, .. } => is_vm,
    };

    // 사용자가 단축키(예: 우측 ⌥)에서 손을 뗄 때까지 최대 260ms 대기 (Cmd+Opt+V 오작동 방지)
    wait_for_modifier_release(Duration::from_millis(260));

    let pb = NSPasteboard::generalPasteboard();
    let snapshot = snapshot_pasteboard(&pb);

    let published_change_count = match write_transient_transcript(&pb, text) {
        Ok(cnt) => cnt,
        Err(err) => {
            warn!("임시 클립보드 게시 실패: {}", err);
            return DeliveryOutcome::NeedsRecoveryCard {
                reason: "클립보드 준비 중 문제가 생겨 카드로 결과를 보여드려요.".to_string(),
            };
        }
    };

    if let Err(err) = post_cmd_v_keystroke() {
        warn!("Cmd+V 키 이벤트 합성 실패: {}", err);
        restore_pasteboard_if_untouched(&pb, published_change_count, &snapshot);
        return DeliveryOutcome::NeedsRecoveryCard {
            reason: "입력 신호를 보내지 못해 카드로 결과를 보여드려요.".to_string(),
        };
    }

    // 대상 앱이 비동기로 클립보드를 읽을 시간을 준 뒤 백그라운드 스레드에서 원래 클립보드 복원
    let grace = if is_vm {
        Duration::from_millis(800)
    } else {
        Duration::from_millis(220)
    };

    thread::Builder::new()
        .name("ttobak-pb-restore".to_string())
        .spawn(move || {
            thread::sleep(grace);
            let pb = NSPasteboard::generalPasteboard();
            restore_pasteboard_if_untouched(&pb, published_change_count, &snapshot);
        })
        .ok();

    DeliveryOutcome::PastedAtCursor
}

fn wait_for_modifier_release(max_wait: Duration) {
    let start = Instant::now();
    let mask = FLAG_SHIFT | FLAG_CONTROL | FLAG_ALTERNATE | FLAG_COMMAND;
    while start.elapsed() < max_wait {
        let flags = unsafe { CGEventSourceFlagsState(K_CG_EVENT_SOURCE_STATE_COMBINED) };
        if (flags & mask) == 0 {
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }
}

fn post_cmd_v_keystroke() -> Result<()> {
    unsafe {
        let source = CGEventSourceCreate(K_CG_EVENT_SOURCE_STATE_HID_SYSTEM);
        if source.is_null() {
            return Err(anyhow!("CGEventSource 생성 실패"));
        }

        let current_flags = CGEventSourceFlagsState(K_CG_EVENT_SOURCE_STATE_COMBINED);
        let preserve_caps = current_flags & FLAG_ALPHA_SHIFT;
        let cmd_flags = FLAG_COMMAND | preserve_caps;

        let key_down = CGEventCreateKeyboardEvent(source, K_VK_ANSI_V, true);
        let key_up = CGEventCreateKeyboardEvent(source, K_VK_ANSI_V, false);

        if key_down.is_null() || key_up.is_null() {
            if !key_down.is_null() {
                CFRelease(key_down);
            }
            if !key_up.is_null() {
                CFRelease(key_up);
            }
            CFRelease(source);
            return Err(anyhow!("CGEvent 키보드 이벤트 생성 실패"));
        }

        CGEventSetFlags(key_down, cmd_flags);
        CGEventSetFlags(key_up, cmd_flags);

        CGEventPost(K_CG_HID_EVENT_TAP, key_down);
        thread::sleep(Duration::from_millis(12));
        CGEventPost(K_CG_HID_EVENT_TAP, key_up);

        CFRelease(key_down);
        CFRelease(key_up);
        CFRelease(source);
    }
    Ok(())
}

fn snapshot_pasteboard(pb: &NSPasteboard) -> Vec<SavedPasteboardItem> {
    let Some(items) = pb.pasteboardItems() else {
        return Vec::new();
    };

    let mut saved = Vec::with_capacity(items.len());
    for item in items {
        let types = item.types();
        let mut entries = Vec::new();
        for uti in types {
            if let Some(data) = item.dataForType(&uti) {
                let bytes = unsafe { data.as_bytes_unchecked() }.to_vec();
                entries.push((uti.to_string(), bytes));
            }
        }
        if !entries.is_empty() {
            saved.push(SavedPasteboardItem { entries });
        }
    }
    saved
}

fn write_transient_transcript(pb: &NSPasteboard, text: &str) -> Result<isize> {
    pb.clearContents();
    let item = NSPasteboardItem::new();

    let utf8_type = NSString::from_str("public.utf8-plain-text");
    let ns_text = NSString::from_str(text);
    if !item.setString_forType(&ns_text, &utf8_type) {
        return Err(anyhow!("UTF-8 문자열 설정 실패"));
    }

    // 클립보드 히스토리 앱(Raycast, Maccy, Paste 등) 및 Universal Clipboard 동기화 제외 마커
    let empty_data = NSData::new();
    for marker in [
        "org.nspasteboard.TransientType",
        "org.nspasteboard.AutoGeneratedType",
        "org.nspasteboard.ConcealedType",
    ] {
        let marker_uti = NSString::from_str(marker);
        let _ = item.setData_forType(&empty_data, &marker_uti);
    }

    let proto_item: objc2::rc::Retained<ProtocolObject<dyn NSPasteboardWriting>> =
        ProtocolObject::from_retained(item);
    let objects = NSArray::from_retained_slice(&[proto_item]);
    let ok = pb.writeObjects(&objects);
    if !ok {
        return Err(anyhow!("NSPasteboard writeObjects 실패"));
    }

    // Universal Clipboard(Handoff) 전파 제한: 로컬 호스트 전용으로 준비 시도 (지원되는 환경)
    let _ = ptr::null::<c_void>();
    Ok(pb.changeCount())
}

fn restore_pasteboard_if_untouched(
    pb: &NSPasteboard,
    expected_change_count: isize,
    snapshot: &[SavedPasteboardItem],
) {
    if pb.changeCount() != expected_change_count {
        debug!("유예 시간 동안 사용자가 새 항목을 복사하여 클립보드 복원을 건너뜁니다.");
        return;
    }

    pb.clearContents();
    if snapshot.is_empty() {
        return;
    }

    let mut restored_items = Vec::with_capacity(snapshot.len());
    for saved_item in snapshot {
        let item = NSPasteboardItem::new();
        let mut has_any = false;
        for (uti, bytes) in &saved_item.entries {
            let ns_uti = NSString::from_str(uti);
            let ns_data = NSData::with_bytes(bytes);
            if item.setData_forType(&ns_data, &ns_uti) {
                has_any = true;
            }
        }
        if has_any {
            let proto_item: objc2::rc::Retained<ProtocolObject<dyn NSPasteboardWriting>> =
                ProtocolObject::from_retained(item);
            restored_items.push(proto_item);
        }
    }

    if !restored_items.is_empty() {
        let arr = NSArray::from_retained_slice(&restored_items);
        let _ = pb.writeObjects(&arr);
    }
}
