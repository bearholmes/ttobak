use block2::RcBlock;
use objc2::msg_send;
use objc2::runtime::{AnyClass, AnyObject};
use parking_lot::Mutex;
use std::ffi::c_void;
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};
use tracing::{debug, info, warn};
use ttobak_core::{ShortcutConfig, ShortcutKind, ShortcutModifiers};

// Carbon HIToolbox 타입 및 상수 (전역 조합 단축키 & 세션 전용 Esc 용 — 별도 TCC 권한 불필요)
type OSStatus = i32;
type EventTargetRef = *mut c_void;
type EventHandlerCallRef = *mut c_void;
type EventRef = *mut c_void;
type EventHandlerRef = *mut c_void;
type EventHotKeyRef = *mut c_void;

#[repr(C)]
#[derive(Clone, Copy)]
struct EventTypeSpec {
    event_class: u32,
    event_kind: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct EventHotKeyID {
    signature: u32,
    id: u32,
}

const K_EVENT_CLASS_KEYBOARD: u32 = 0x6b65_7962; // 'keyb'
const K_EVENT_PARAM_DIRECT_OBJECT: u32 = 0x2d2d_2d2d; // '----'
const TYPE_EVENT_HOT_KEY_ID: u32 = 0x686b_6964; // 'hkid'
const K_EVENT_HOT_KEY_PRESSED: u32 = 5;
const K_EVENT_HOT_KEY_RELEASED: u32 = 6;

const CARBON_CMD_KEY: u32 = 1 << 8; // 0x0100
const CARBON_SHIFT_KEY: u32 = 1 << 9; // 0x0200
const CARBON_OPTION_KEY: u32 = 1 << 11; // 0x0800
const CARBON_CONTROL_KEY: u32 = 1 << 12; // 0x1000

const HOTKEY_SIGNATURE: u32 = 0x5454_424b; // 'TTBK'
const HOTKEY_ID_TOGGLE_COMBO: u32 = 1;
const HOTKEY_ID_ACTIVE_ESC: u32 = 2;

// NSEventMask 상수
const NS_EVENT_MASK_KEY_DOWN: u64 = 1 << 10;
const NS_EVENT_MASK_FLAGS_CHANGED: u64 = 1 << 12;

// macOS 가상 키코드
const KEYCODE_ESCAPE: u16 = 53;
const KEYCODE_RIGHT_COMMAND: u16 = 54;
const KEYCODE_LEFT_OPTION: u16 = 58;
const KEYCODE_RIGHT_OPTION: u16 = 61;
const KEYCODE_RIGHT_CONTROL: u16 = 62;
const KEYCODE_FN: u16 = 63;

// IOKit / NX 장치별 좌·우 수식키 하위 비트마스크 (NSEvent.modifierFlags에 포함됨)
const NX_DEVICELALTKEYMASK: u64 = 0x0000_0020;
const NX_DEVICERALTKEYMASK: u64 = 0x0000_0040;
const NX_DEVICERCMDKEYMASK: u64 = 0x0000_0010;
const NX_DEVICERCTLKEYMASK: u64 = 0x0000_2000;

// NSEventModifierFlags 상위 비트마스크
const FLAG_SHIFT: u64 = 0x0002_0000;
const FLAG_CONTROL: u64 = 0x0004_0000;
const FLAG_ALTERNATE: u64 = 0x0008_0000;
const FLAG_COMMAND: u64 = 0x0010_0000;
const FLAG_SECONDARY_FN: u64 = 0x0080_0000;

#[repr(C)]
struct DispatchQueueOpaque([u8; 0]);

extern "C" {
    static _dispatch_main_q: DispatchQueueOpaque;
    fn dispatch_async_f(
        queue: *const DispatchQueueOpaque,
        context: *mut c_void,
        work: extern "C" fn(*mut c_void),
    );
    fn pthread_main_np() -> i32;
}

#[link(name = "Carbon", kind = "framework")]
extern "C" {
    fn GetApplicationEventTarget() -> EventTargetRef;
    fn InstallEventHandler(
        in_target: EventTargetRef,
        in_handler: extern "C" fn(EventHandlerCallRef, EventRef, *mut c_void) -> OSStatus,
        in_num_types: usize,
        in_list: *const EventTypeSpec,
        in_user_data: *mut c_void,
        out_ref: *mut EventHandlerRef,
    ) -> OSStatus;
    fn RegisterEventHotKey(
        in_hot_key_code: u32,
        in_hot_key_modifiers: u32,
        in_hot_key_id: EventHotKeyID,
        in_target: EventTargetRef,
        in_options: u32,
        out_ref: *mut EventHotKeyRef,
    ) -> OSStatus;
    fn UnregisterEventHotKey(in_hot_key: EventHotKeyRef) -> OSStatus;
    fn GetEventKind(in_event: EventRef) -> u32;
    fn GetEventParameter(
        in_event: EventRef,
        in_name: u32,
        in_desired_type: u32,
        out_actual_type: *mut u32,
        in_buffer_size: usize,
        out_actual_size: *mut usize,
        out_data: *mut c_void,
    ) -> OSStatus;
}

fn run_on_macos_main_thread<F: FnOnce() + Send + 'static>(task: F) {
    if unsafe { pthread_main_np() } != 0 {
        task();
        return;
    }

    extern "C" fn trampoline<F: FnOnce()>(raw: *mut c_void) {
        let boxed = unsafe { Box::from_raw(raw as *mut F) };
        boxed();
    }

    let boxed: Box<F> = Box::new(task);
    unsafe {
        dispatch_async_f(
            &_dispatch_main_q as *const DispatchQueueOpaque,
            Box::into_raw(boxed) as *mut c_void,
            trampoline::<F>,
        );
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyEvent {
    /// 말하기 시작/종료 토글 단축키 발동
    ToggleTriggered,
    /// 활성 세션 중 `Esc` 단독 입력으로 취소 발동
    CancelTriggered,
}

struct TapRuntimeState {
    config: ShortcutConfig,
    modifier_down_at: Option<Instant>,
    chord_blocked: bool,
    hold_fired_for_current_press: bool,
    combo_down_at: Option<Instant>,
    combo_fired: bool,
}

struct CarbonRegistrationState {
    handler_installed: bool,
    combo_hotkey_ref: usize,
    registered_combo: Option<(u16, ShortcutModifiers)>,
    esc_hotkey_ref: usize,
}

struct SharedTapContext {
    state: Mutex<TapRuntimeState>,
    carbon: Mutex<CarbonRegistrationState>,
    session_active: AtomicBool,
    listening_paused: AtomicBool,
    callback: Box<dyn Fn(HotkeyEvent) + Send + Sync + 'static>,
}

/// 전역 단축키 매니저
/// - 단독 수식키(`우측 ⌥` 등): `NSEvent` Global + Local `FlagsChanged` 모니터 (권한 없이도 즉시 동작)
/// - 일반 조합키(`⌃ ⌥ Space` 등) & 활성 세션 전용 `Esc`: Carbon `RegisterEventHotKey` (권한 없이도 전역 동작)
pub struct GlobalHotkeyManager {
    ctx: Arc<SharedTapContext>,
}

impl GlobalHotkeyManager {
    pub fn start<F>(initial_config: ShortcutConfig, on_event: F) -> Self
    where
        F: Fn(HotkeyEvent) + Send + Sync + 'static,
    {
        let ctx = Arc::new(SharedTapContext {
            state: Mutex::new(TapRuntimeState {
                config: initial_config,
                modifier_down_at: None,
                chord_blocked: false,
                hold_fired_for_current_press: false,
                combo_down_at: None,
                combo_fired: false,
            }),
            carbon: Mutex::new(CarbonRegistrationState {
                handler_installed: false,
                combo_hotkey_ref: 0,
                registered_combo: None,
                esc_hotkey_ref: 0,
            }),
            session_active: AtomicBool::new(false),
            listening_paused: AtomicBool::new(false),
            callback: Box::new(on_event),
        });

        let main_ctx = Arc::clone(&ctx);
        run_on_macos_main_thread(move || {
            install_nsevent_monitors_on_main(Arc::clone(&main_ctx));
            install_carbon_handler_on_main(&main_ctx);
            sync_carbon_hotkeys_on_main(&main_ctx);
        });

        // 홀드 시간(0.5초 / 1.0초) 타이머 체크 워커
        let timer_ctx = Arc::clone(&ctx);
        thread::Builder::new()
            .name("ttobak-hotkey-hold-timer".to_string())
            .spawn(move || loop {
                thread::sleep(Duration::from_millis(25));
                if timer_ctx.listening_paused.load(Ordering::Relaxed) {
                    continue;
                }
                let should_fire = {
                    let mut st = timer_ctx.state.lock();
                    let hold_ms = st.config.hold_ms;
                    if hold_ms == 0 {
                        false
                    } else {
                        let dur = Duration::from_millis(hold_ms as u64);
                        let mod_ready = st
                            .modifier_down_at
                            .map(|t| {
                                !st.chord_blocked
                                    && !st.hold_fired_for_current_press
                                    && t.elapsed() >= dur
                            })
                            .unwrap_or(false);
                        let combo_ready = st
                            .combo_down_at
                            .map(|t| !st.combo_fired && t.elapsed() >= dur)
                            .unwrap_or(false);

                        if mod_ready {
                            st.hold_fired_for_current_press = true;
                            true
                        } else if combo_ready {
                            st.combo_fired = true;
                            true
                        } else {
                            false
                        }
                    }
                };
                if should_fire {
                    (timer_ctx.callback)(HotkeyEvent::ToggleTriggered);
                }
            })
            .ok();

        Self { ctx }
    }

    pub fn update_config(&self, new_config: ShortcutConfig) {
        {
            let mut st = self.ctx.state.lock();
            st.config = new_config;
            st.modifier_down_at = None;
            st.chord_blocked = false;
            st.hold_fired_for_current_press = false;
            st.combo_down_at = None;
            st.combo_fired = false;
        }
        let c = Arc::clone(&self.ctx);
        run_on_macos_main_thread(move || {
            sync_carbon_hotkeys_on_main(&c);
        });
    }

    /// 녹음/변환/복구 세션 활성화 여부를 갱신합니다 (`true`일 때만 전역 단독 `Esc` 핫키 동적 등록).
    pub fn set_session_active(&self, active: bool) {
        let prev = self.ctx.session_active.swap(active, Ordering::SeqCst);
        if prev != active {
            let c = Arc::clone(&self.ctx);
            run_on_macos_main_thread(move || {
                sync_carbon_hotkeys_on_main(&c);
            });
        }
    }

    /// 설정 창에서 새 단축키를 녹화하는 동안 기존 단축키 발동을 일시 정지합니다.
    pub fn set_paused(&self, paused: bool) {
        let prev = self.ctx.listening_paused.swap(paused, Ordering::SeqCst);
        if prev != paused {
            let c = Arc::clone(&self.ctx);
            run_on_macos_main_thread(move || {
                sync_carbon_hotkeys_on_main(&c);
            });
        }
    }
}

fn install_nsevent_monitors_on_main(ctx: Arc<SharedTapContext>) {
    unsafe {
        let Some(ns_event_cls) = AnyClass::get(c"NSEvent") else {
            warn!("NSEvent 클래스를 찾을 수 없습니다");
            return;
        };

        // 1. 전역 FlagsChanged 모니터 (다른 모든 앱 포커스 중 단독 수식키 감지 — TCC 권한 불필요)
        let ctx_global_flags = Arc::clone(&ctx);
        let global_flags_block = RcBlock::new(move |event: *mut AnyObject| {
            if !event.is_null() {
                let keycode: u16 = msg_send![event, keyCode];
                let flags: usize = msg_send![event, modifierFlags];
                handle_flags_changed(&ctx_global_flags, keycode, flags as u64);
            }
        });
        let mon1: *mut AnyObject = msg_send![
            ns_event_cls,
            addGlobalMonitorForEventsMatchingMask: NS_EVENT_MASK_FLAGS_CHANGED,
            handler: &*global_flags_block
        ];
        if !mon1.is_null() {
            let _: *mut AnyObject = msg_send![mon1, retain];
        }
        std::mem::forget(global_flags_block);

        // 2. 로컬 FlagsChanged 모니터 (또박 설정/팝오버 창이 포커스된 상태에서도 동일하게 단독 수식키 감지)
        let ctx_local_flags = Arc::clone(&ctx);
        let local_flags_block = RcBlock::new(move |event: *mut AnyObject| -> *mut AnyObject {
            if !event.is_null() {
                let keycode: u16 = msg_send![event, keyCode];
                let flags: usize = msg_send![event, modifierFlags];
                handle_flags_changed(&ctx_local_flags, keycode, flags as u64);
            }
            event
        });
        let mon2: *mut AnyObject = msg_send![
            ns_event_cls,
            addLocalMonitorForEventsMatchingMask: NS_EVENT_MASK_FLAGS_CHANGED,
            handler: &*local_flags_block
        ];
        if !mon2.is_null() {
            let _: *mut AnyObject = msg_send![mon2, retain];
        }
        std::mem::forget(local_flags_block);

        // 3. 전역 KeyDown 모니터 (우측 ⌥ 등을 누른 채 일반 키를 함께 누르는 조합 입력 차단용)
        let ctx_global_kd = Arc::clone(&ctx);
        let global_kd_block = RcBlock::new(move |event: *mut AnyObject| {
            if !event.is_null() {
                let keycode: u16 = msg_send![event, keyCode];
                let flags: usize = msg_send![event, modifierFlags];
                let is_repeat: bool = msg_send![event, isARepeat];
                let _ = handle_key_down(&ctx_global_kd, keycode, flags as u64, is_repeat);
            }
        });
        let mon3: *mut AnyObject = msg_send![
            ns_event_cls,
            addGlobalMonitorForEventsMatchingMask: NS_EVENT_MASK_KEY_DOWN,
            handler: &*global_kd_block
        ];
        if !mon3.is_null() {
            let _: *mut AnyObject = msg_send![mon3, retain];
        }
        std::mem::forget(global_kd_block);

        // 4. 로컬 KeyDown 모니터 (또박 창 내부에서의 조합 차단 및 활성 세션 중 Esc 즉시 취소)
        let ctx_local_kd = Arc::clone(&ctx);
        let local_kd_block = RcBlock::new(move |event: *mut AnyObject| -> *mut AnyObject {
            if !event.is_null() {
                let keycode: u16 = msg_send![event, keyCode];
                let flags: usize = msg_send![event, modifierFlags];
                let is_repeat: bool = msg_send![event, isARepeat];
                if handle_key_down(&ctx_local_kd, keycode, flags as u64, is_repeat) {
                    return ptr::null_mut();
                }
            }
            event
        });
        let mon4: *mut AnyObject = msg_send![
            ns_event_cls,
            addLocalMonitorForEventsMatchingMask: NS_EVENT_MASK_KEY_DOWN,
            handler: &*local_kd_block
        ];
        if !mon4.is_null() {
            let _: *mut AnyObject = msg_send![mon4, retain];
        }
        std::mem::forget(local_kd_block);

        info!("NSEvent 전역/로컬 단축키 모니터 활성화 완료");
    }
}

fn handle_flags_changed(ctx: &SharedTapContext, keycode: u16, flags: u64) {
    if ctx.listening_paused.load(Ordering::Relaxed) {
        return;
    }

    let mut fire_toggle = false;
    {
        let mut st = ctx.state.lock();
        let hold_ms = st.config.hold_ms;
        match st.config.kind {
            ShortcutKind::RightOption
            | ShortcutKind::LeftOption
            | ShortcutKind::RightCommand
            | ShortcutKind::RightControl
            | ShortcutKind::FnKey => {
                if let Some(is_target_down) =
                    standalone_modifier_pressed(&st.config.kind, keycode, flags)
                {
                    if is_target_down {
                        let other_mods = has_other_modifiers(&st.config.kind, flags);
                        st.modifier_down_at = Some(Instant::now());
                        st.chord_blocked = other_mods;
                        st.hold_fired_for_current_press = false;
                    } else if let Some(down_at) = st.modifier_down_at.take() {
                        if !st.chord_blocked
                            && !st.hold_fired_for_current_press
                            && hold_ms == 0
                            && down_at.elapsed() < Duration::from_secs(5)
                        {
                            fire_toggle = true;
                        }
                        st.chord_blocked = false;
                        st.hold_fired_for_current_press = false;
                    }
                } else if st.modifier_down_at.is_some() {
                    // 대상 수식키를 누른 상태에서 다른 수식키를 추가로 누르면 오작동 방지를 위해 차단
                    st.chord_blocked = true;
                }
            }
            ShortcutKind::KeyCombo { .. } => {}
        }
    }

    if fire_toggle {
        debug!("단독 수식키 단축키 토글 발동 (keyCode={})", keycode);
        (ctx.callback)(HotkeyEvent::ToggleTriggered);
    }
}

/// 로컬 모니터에서 이벤트를 소비해야 하면 `true`를 반환합니다.
fn handle_key_down(ctx: &SharedTapContext, keycode: u16, flags: u64, is_repeat: bool) -> bool {
    if ctx.listening_paused.load(Ordering::Relaxed) {
        return false;
    }

    // 수식키 홀드 중 일반 키 입력이 들어오면 조합 단축키(예: ⌥+화살표)로 간주하고 토글 차단
    {
        let mut st = ctx.state.lock();
        if st.modifier_down_at.is_some() {
            st.chord_blocked = true;
        }
    }

    // 로컬 창 포커스 중 활성 세션 Esc 백업 처리 (전역 Esc는 Carbon 핫키가 가로챔)
    if !is_repeat && keycode == KEYCODE_ESCAPE && ctx.session_active.load(Ordering::Relaxed) {
        let any_mod = (flags & (FLAG_COMMAND | FLAG_ALTERNATE | FLAG_CONTROL)) != 0;
        if !any_mod {
            debug!("로컬 키 모니터에서 활성 세션 Esc 취소 감지");
            (ctx.callback)(HotkeyEvent::CancelTriggered);
            return true;
        }
    }

    false
}

fn install_carbon_handler_on_main(ctx: &Arc<SharedTapContext>) {
    let mut carbon = ctx.carbon.lock();
    if carbon.handler_installed {
        return;
    }

    let specs = [
        EventTypeSpec {
            event_class: K_EVENT_CLASS_KEYBOARD,
            event_kind: K_EVENT_HOT_KEY_PRESSED,
        },
        EventTypeSpec {
            event_class: K_EVENT_CLASS_KEYBOARD,
            event_kind: K_EVENT_HOT_KEY_RELEASED,
        },
    ];

    // 프로세스 수명 동안 유지되도록 참조 카운트 1개를 고정
    let leaked_ctx_ptr = Arc::into_raw(Arc::clone(ctx)) as *mut c_void;
    let mut handler_ref: EventHandlerRef = ptr::null_mut();

    let status = unsafe {
        InstallEventHandler(
            GetApplicationEventTarget(),
            carbon_hotkey_callback,
            specs.len(),
            specs.as_ptr(),
            leaked_ctx_ptr,
            &mut handler_ref,
        )
    };

    if status == 0 {
        carbon.handler_installed = true;
        info!("Carbon 전역 핫키 핸들러 등록 완료");
    } else {
        warn!("Carbon InstallEventHandler 실패: OSStatus={}", status);
    }
}

fn sync_carbon_hotkeys_on_main(ctx: &SharedTapContext) {
    let paused = ctx.listening_paused.load(Ordering::SeqCst);
    let session_active = ctx.session_active.load(Ordering::SeqCst);
    let desired_combo = if paused {
        None
    } else {
        match ctx.state.lock().config.kind.clone() {
            ShortcutKind::KeyCombo {
                key_code,
                modifiers,
                ..
            } => Some((key_code, modifiers)),
            _ => None,
        }
    };

    let mut carbon = ctx.carbon.lock();

    // 1. 일반 키 조합 단축키 동기화
    if carbon.registered_combo != desired_combo {
        if carbon.combo_hotkey_ref != 0 {
            unsafe {
                let _ = UnregisterEventHotKey(carbon.combo_hotkey_ref as EventHotKeyRef);
            }
            carbon.combo_hotkey_ref = 0;
            carbon.registered_combo = None;
        }

        if let Some((kc, mods)) = desired_combo {
            let mut hk_ref: EventHotKeyRef = ptr::null_mut();
            let hk_id = EventHotKeyID {
                signature: HOTKEY_SIGNATURE,
                id: HOTKEY_ID_TOGGLE_COMBO,
            };
            let status = unsafe {
                RegisterEventHotKey(
                    kc as u32,
                    to_carbon_modifiers(&mods),
                    hk_id,
                    GetApplicationEventTarget(),
                    0,
                    &mut hk_ref,
                )
            };
            if status == 0 && !hk_ref.is_null() {
                carbon.combo_hotkey_ref = hk_ref as usize;
                carbon.registered_combo = Some((kc, mods));
                info!("조합형 전역 단축키 등록 완료 (keyCode={})", kc);
            } else {
                warn!("RegisterEventHotKey(조합키) 실패: OSStatus={}", status);
            }
        }
    }

    // 2. 활성 세션 전용 단독 Esc 핫키 동적 등록/해제 (`registerBareEscapeForActiveSession`)
    let want_esc = session_active && !paused;
    if want_esc && carbon.esc_hotkey_ref == 0 {
        let mut esc_ref: EventHotKeyRef = ptr::null_mut();
        let hk_id = EventHotKeyID {
            signature: HOTKEY_SIGNATURE,
            id: HOTKEY_ID_ACTIVE_ESC,
        };
        let status = unsafe {
            RegisterEventHotKey(
                KEYCODE_ESCAPE as u32,
                0,
                hk_id,
                GetApplicationEventTarget(),
                0,
                &mut esc_ref,
            )
        };
        if status == 0 && !esc_ref.is_null() {
            carbon.esc_hotkey_ref = esc_ref as usize;
            debug!("활성 세션 전역 Esc 핫키 등록됨");
        } else {
            warn!("RegisterEventHotKey(Esc) 실패: OSStatus={}", status);
        }
    } else if !want_esc && carbon.esc_hotkey_ref != 0 {
        unsafe {
            let _ = UnregisterEventHotKey(carbon.esc_hotkey_ref as EventHotKeyRef);
        }
        carbon.esc_hotkey_ref = 0;
        debug!("활성 세션 종료 -> 전역 Esc 핫키 해제됨");
    }
}

extern "C" fn carbon_hotkey_callback(
    _next_handler: EventHandlerCallRef,
    event: EventRef,
    user_data: *mut c_void,
) -> OSStatus {
    if event.is_null() || user_data.is_null() {
        return 0;
    }

    let ctx = unsafe { &*(user_data as *const SharedTapContext) };
    if ctx.listening_paused.load(Ordering::Relaxed) {
        return 0;
    }

    let mut hk_id = EventHotKeyID {
        signature: 0,
        id: 0,
    };
    let status = unsafe {
        GetEventParameter(
            event,
            K_EVENT_PARAM_DIRECT_OBJECT,
            TYPE_EVENT_HOT_KEY_ID,
            ptr::null_mut(),
            std::mem::size_of::<EventHotKeyID>(),
            ptr::null_mut(),
            &mut hk_id as *mut EventHotKeyID as *mut c_void,
        )
    };
    if status != 0 || hk_id.signature != HOTKEY_SIGNATURE {
        return 0;
    }

    let kind = unsafe { GetEventKind(event) };

    match hk_id.id {
        HOTKEY_ID_ACTIVE_ESC => {
            if kind == K_EVENT_HOT_KEY_PRESSED && ctx.session_active.load(Ordering::Relaxed) {
                debug!("전역 Carbon Esc 핫키 감지 -> 세션 취소");
                (ctx.callback)(HotkeyEvent::CancelTriggered);
            }
        }
        HOTKEY_ID_TOGGLE_COMBO => {
            if kind == K_EVENT_HOT_KEY_PRESSED {
                let fire_now = {
                    let mut st = ctx.state.lock();
                    if st.config.hold_ms == 0 {
                        true
                    } else {
                        st.combo_down_at = Some(Instant::now());
                        st.combo_fired = false;
                        false
                    }
                };
                if fire_now {
                    (ctx.callback)(HotkeyEvent::ToggleTriggered);
                }
            } else if kind == K_EVENT_HOT_KEY_RELEASED {
                let mut st = ctx.state.lock();
                st.combo_down_at = None;
                st.combo_fired = false;
            }
        }
        _ => {}
    }

    0
}

fn to_carbon_modifiers(m: &ShortcutModifiers) -> u32 {
    let mut flags = 0u32;
    if m.command {
        flags |= CARBON_CMD_KEY;
    }
    if m.option {
        flags |= CARBON_OPTION_KEY;
    }
    if m.control {
        flags |= CARBON_CONTROL_KEY;
    }
    if m.shift {
        flags |= CARBON_SHIFT_KEY;
    }
    flags
}

fn standalone_modifier_pressed(kind: &ShortcutKind, keycode: u16, flags: u64) -> Option<bool> {
    match kind {
        ShortcutKind::RightOption if keycode == KEYCODE_RIGHT_OPTION => {
            let down = if (flags & NX_DEVICERALTKEYMASK) != 0 {
                true
            } else {
                (flags & FLAG_ALTERNATE) != 0 && (flags & NX_DEVICELALTKEYMASK) == 0
            };
            Some(down)
        }
        ShortcutKind::LeftOption if keycode == KEYCODE_LEFT_OPTION => {
            let down = if (flags & NX_DEVICELALTKEYMASK) != 0 {
                true
            } else {
                (flags & FLAG_ALTERNATE) != 0 && (flags & NX_DEVICERALTKEYMASK) == 0
            };
            Some(down)
        }
        ShortcutKind::RightCommand if keycode == KEYCODE_RIGHT_COMMAND => {
            let down = (flags & NX_DEVICERCMDKEYMASK) != 0 || (flags & FLAG_COMMAND) != 0;
            Some(down)
        }
        ShortcutKind::RightControl if keycode == KEYCODE_RIGHT_CONTROL => {
            let down = (flags & NX_DEVICERCTLKEYMASK) != 0 || (flags & FLAG_CONTROL) != 0;
            Some(down)
        }
        ShortcutKind::FnKey if keycode == KEYCODE_FN => Some((flags & FLAG_SECONDARY_FN) != 0),
        _ => None,
    }
}

fn has_other_modifiers(kind: &ShortcutKind, flags: u64) -> bool {
    let cmd = (flags & FLAG_COMMAND) != 0;
    let opt = (flags & FLAG_ALTERNATE) != 0;
    let ctrl = (flags & FLAG_CONTROL) != 0;
    let shift = (flags & FLAG_SHIFT) != 0;

    match kind {
        ShortcutKind::RightOption | ShortcutKind::LeftOption => cmd || ctrl || shift,
        ShortcutKind::RightCommand => opt || ctrl || shift,
        ShortcutKind::RightControl => cmd || opt || shift,
        ShortcutKind::FnKey => cmd || opt || ctrl || shift,
        ShortcutKind::KeyCombo { .. } => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_right_option_detection_distinguishes_left_option() {
        assert_eq!(
            standalone_modifier_pressed(
                &ShortcutKind::RightOption,
                KEYCODE_RIGHT_OPTION,
                FLAG_ALTERNATE | NX_DEVICERALTKEYMASK
            ),
            Some(true)
        );
        assert_eq!(
            standalone_modifier_pressed(
                &ShortcutKind::RightOption,
                KEYCODE_LEFT_OPTION,
                FLAG_ALTERNATE | NX_DEVICELALTKEYMASK
            ),
            None
        );
        assert_eq!(
            standalone_modifier_pressed(&ShortcutKind::RightOption, KEYCODE_RIGHT_OPTION, 0),
            Some(false)
        );
    }

    #[test]
    fn test_to_carbon_modifiers_mapping() {
        let m = ShortcutModifiers {
            command: true,
            option: true,
            control: true,
            shift: false,
        };
        assert_eq!(
            to_carbon_modifiers(&m),
            CARBON_CMD_KEY | CARBON_OPTION_KEY | CARBON_CONTROL_KEY
        );
    }
}
