// Razer Viper V3 Pro battery tray. No console window.
#![windows_subsystem = "windows"]

mod battery;
mod icon;
mod popup;

use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::Arc;
use std::thread;

use muda::{Menu, MenuEvent, MenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::*;
use windows::Win32::System::Threading::*;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
use windows::Win32::UI::WindowsAndMessaging::*;

const REFRESH_SECS: i64 = 300;
const TRANSIENT_RETRY_SECS: i64 = 15;
const OFFLINE_RETRY_SECS: i64 = 30;
const OFFLINE_AFTER_MISSES: u8 = 3;
const ACTIVITY_POLL_SECS: i64 = 3;
const APP_TITLE: &str = "Viper Tray";

const STATE_CHECKING: i32 = -1;
const STATE_CONNECTED: i32 = 0;
const STATE_RETRYING: i32 = 1;
const STATE_OFFLINE: i32 = 2;

// HANDLE is *mut c_void which isn't Send by default. We share kernel handles between
// the UI thread and the worker thread; that's safe — Win32 sync handles are designed
// for cross-thread use.
#[derive(Clone, Copy)]
struct SendHandle(HANDLE);
unsafe impl Send for SendHandle {}
unsafe impl Sync for SendHandle {}

const WM_APP_REFRESH: u32 = WM_APP + 1;
const WM_APP_TRAY_LCLICK: u32 = WM_APP + 2;
const WM_APP_MENU: u32 = WM_APP + 3;

static MAIN_THREAD_ID: AtomicI32 = AtomicI32::new(0);

fn main() {
    // Single-instance: Local\-scoped named mutex (no privilege required).
    let _guard = match acquire_singleton() {
        Some(h) => h,
        None => return, // another instance is already running
    };

    MAIN_THREAD_ID.store(unsafe { GetCurrentThreadId() } as i32, Ordering::Release);

    // Build tray + menu
    let menu = Menu::new();
    let item_refresh = MenuItem::new("Refresh now", true, None);
    let item_exit = MenuItem::new("Exit", true, None);
    menu.append(&item_refresh).unwrap();
    menu.append(&item_exit).unwrap();
    let id_refresh = item_refresh.id().clone();
    let id_exit = item_exit.id().clone();

    let last_pct = Arc::new(AtomicI32::new(-1)); // -1 = no successful read yet
    let connection_state = Arc::new(AtomicI32::new(STATE_CHECKING));

    let tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_menu_on_left_click(false) // left-click = our popup, right-click = menu
        .with_tooltip(format!("{}: checking...", APP_TITLE))
        .with_icon(build_icon(None))
        .build()
        .expect("failed to build tray");

    // Waitable timer: drives both periodic and manual refreshes.
    // Manual refresh = SetWaitableTimer(due_time = -1ms) to fire immediately.
    let timer = SendHandle(
        unsafe { CreateWaitableTimerW(None, false, PCWSTR::null()) }.expect("CreateWaitableTimer"),
    );
    arm_timer(timer, 0); // first read fires immediately on startup

    // Worker: waits on the timer, queries the mouse, posts WM_APP_REFRESH to UI thread.
    {
        let last_pct = last_pct.clone();
        let connection_state = connection_state.clone();
        thread::spawn(move || {
            let mut misses: u8 = 0;
            // After a failed read, poll for user input instead of sleeping blind:
            // the mouse is usually just asleep, and any activity means it's awake again.
            let mut fallback_secs: i64 = 0;
            let mut waited_secs: i64 = 0;
            let mut seen_input = last_input_tick();
            loop {
                // Block until the timer fires (system-coalesced).
                wait_handle(timer);
                if fallback_secs > 0 {
                    waited_secs += ACTIVITY_POLL_SECS;
                    if last_input_tick() == seen_input && waited_secs < fallback_secs {
                        arm_timer(timer, ACTIVITY_POLL_SECS);
                        continue;
                    }
                }
                seen_input = last_input_tick();
                waited_secs = 0;
                let next_refresh_secs = match battery::read_battery_percent() {
                    Some(val) => {
                        misses = 0;
                        fallback_secs = 0;
                        last_pct.store(val as i32, Ordering::Release);
                        connection_state.store(STATE_CONNECTED, Ordering::Release);
                        REFRESH_SECS
                    }
                    None => {
                        misses = misses.saturating_add(1);
                        if misses < OFFLINE_AFTER_MISSES {
                            connection_state.store(STATE_RETRYING, Ordering::Release);
                            fallback_secs = TRANSIENT_RETRY_SECS;
                        } else {
                            connection_state.store(STATE_OFFLINE, Ordering::Release);
                            fallback_secs = OFFLINE_RETRY_SECS;
                        }
                        ACTIVITY_POLL_SECS
                    }
                };
                unsafe {
                    let _ = PostThreadMessageW(
                        MAIN_THREAD_ID.load(Ordering::Acquire) as u32,
                        WM_APP_REFRESH,
                        WPARAM(0),
                        LPARAM(0),
                    );
                }
                // Re-arm for the next periodic check.
                arm_timer(timer, next_refresh_secs);
            }
        });
    }

    // Tray-icon event handler: route left-click into our message pump.
    TrayIconEvent::set_event_handler(Some(|event: TrayIconEvent| {
        if let TrayIconEvent::Click {
            button: MouseButton::Left,
            button_state: MouseButtonState::Up,
            ..
        } = event
        {
            unsafe {
                let _ = PostThreadMessageW(
                    MAIN_THREAD_ID.load(Ordering::Acquire) as u32,
                    WM_APP_TRAY_LCLICK,
                    WPARAM(0),
                    LPARAM(0),
                );
            }
        }
    }));

    // Menu event handler: encode item id into LPARAM.
    let menu_event = Arc::new(AtomicI32::new(0));
    {
        let id_refresh = id_refresh.clone();
        let id_exit = id_exit.clone();
        let menu_event = menu_event.clone();
        MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
            let v = if event.id == id_refresh {
                1
            } else if event.id == id_exit {
                2
            } else {
                0
            };
            if v != 0 {
                menu_event.store(v, Ordering::Release);
                unsafe {
                    let _ = PostThreadMessageW(
                        MAIN_THREAD_ID.load(Ordering::Acquire) as u32,
                        WM_APP_MENU,
                        WPARAM(0),
                        LPARAM(0),
                    );
                }
            }
        }));
    }

    // Main message loop on the UI thread.
    unsafe {
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            match msg.message {
                m if m == WM_APP_REFRESH => {
                    let display = load_display(&last_pct, &connection_state);
                    let _ = tray.set_icon(Some(build_icon(display.icon_percent())));
                    let _ = tray.set_tooltip(Some(display.tooltip()));
                }
                m if m == WM_APP_TRAY_LCLICK => {
                    let display = load_display(&last_pct, &connection_state);
                    popup::toggle_popup(display.popup_percent(), display.popup_state());
                    // fire a fresh read so the next click shows newer data
                    arm_timer(timer, 0);
                }
                m if m == WM_APP_MENU => {
                    let v = menu_event.swap(0, Ordering::AcqRel);
                    if v == 1 {
                        arm_timer(timer, 0);
                    } else if v == 2 {
                        PostQuitMessage(0);
                    }
                }
                _ => {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
        }
    }

    unsafe {
        let _ = CloseHandle(timer.0);
    }
}

fn arm_timer(h: SendHandle, secs: i64) {
    // Negative = relative, units = 100ns. 0 secs => fire immediately (-1 == 100ns).
    let due_ns: i64 = if secs <= 0 { -1 } else { -secs * 10_000_000 };
    // Loud-fail: if the timer can't be armed, the worker thread will block forever
    // and the tray will go stale silently. Better to crash than hang invisibly.
    unsafe {
        SetWaitableTimer(h.0, &due_ns, 0, None, None, false)
            .expect("SetWaitableTimer failed — worker would be dead");
    }
}

fn last_input_tick() -> u32 {
    let mut info = LASTINPUTINFO {
        cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32,
        dwTime: 0,
    };
    unsafe {
        let _ = GetLastInputInfo(&mut info);
    }
    info.dwTime
}

fn wait_handle(h: SendHandle) {
    unsafe {
        let _ = WaitForSingleObject(h.0, INFINITE);
    }
}

fn build_icon(pct: Option<u8>) -> Icon {
    let (w, h) = icon::icon_dim();
    let rgba = icon::build_icon_rgba(pct);
    Icon::from_rgba(rgba, w, h).expect("icon")
}

#[derive(Clone, Copy)]
struct DisplayState {
    last_pct: Option<u8>,
    state: i32,
}

impl DisplayState {
    fn icon_percent(self) -> Option<u8> {
        match self.state {
            STATE_CONNECTED | STATE_RETRYING => self.last_pct,
            _ => None,
        }
    }

    fn popup_percent(self) -> Option<u8> {
        match self.state {
            STATE_CONNECTED | STATE_RETRYING => self.last_pct,
            _ => None,
        }
    }

    fn popup_state(self) -> popup::PopupState {
        match self.state {
            STATE_CHECKING => popup::PopupState::Checking,
            STATE_CONNECTED => popup::PopupState::Connected,
            STATE_RETRYING => popup::PopupState::Retrying,
            _ => popup::PopupState::Offline,
        }
    }

    fn tooltip(self) -> String {
        match (self.state, self.last_pct) {
            (STATE_CHECKING, _) => format!("{}: checking...", APP_TITLE),
            (STATE_CONNECTED, Some(p)) => format!("{}: {}%", APP_TITLE, p),
            (STATE_RETRYING, Some(p)) => format!("{}: {}% (retrying)", APP_TITLE, p),
            (STATE_RETRYING, None) => format!("{}: retrying...", APP_TITLE),
            (STATE_OFFLINE, Some(p)) => format!("{}: offline (last seen {}%)", APP_TITLE, p),
            _ => format!("{}: offline", APP_TITLE),
        }
    }
}

fn load_display(last_pct: &AtomicI32, connection_state: &AtomicI32) -> DisplayState {
    let raw_pct = last_pct.load(Ordering::Acquire);
    let last_pct = if raw_pct < 0 {
        None
    } else {
        Some(raw_pct as u8)
    };
    DisplayState {
        last_pct,
        state: connection_state.load(Ordering::Acquire),
    }
}

fn acquire_singleton() -> Option<HANDLE> {
    unsafe {
        // CreateMutexW returns a valid handle even when the mutex already exists;
        // GetLastError() distinguishes the two cases. Local\ scope = no extra privilege.
        let h = match CreateMutexW(None, true, w!("Local\\ViperTray-singleton")) {
            Ok(h) => h,
            Err(_) => return None,
        };
        if GetLastError() == ERROR_ALREADY_EXISTS {
            let _ = CloseHandle(h);
            return None;
        }
        Some(h)
    }
}
