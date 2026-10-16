// Tiny borderless popup window near the cursor, like a tray flyout.
// Uses SetCapture for proper click-anywhere-to-dismiss; GWLP_USERDATA for the
// per-window display payload (no static mut). Clamps position to the monitor work area.

use std::ptr::null_mut;
use std::sync::atomic::{AtomicPtr, Ordering};

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture};
use windows::Win32::UI::WindowsAndMessaging::*;

use std::ffi::c_void;

static OPEN_POPUP_HWND: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());
static CLASS_REGISTERED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

const POPUP_W: i32 = 196;
const POPUP_H: i32 = 72;

#[derive(Clone, Copy)]
pub enum PopupState {
    Checking,
    Connected,
    Retrying,
    Offline,
}

struct PopupData {
    percent: Option<u8>,
    state: PopupState,
}

pub fn toggle_popup(percent: Option<u8>, state: PopupState) {
    unsafe {
        // Toggle off if already showing.
        let existing = OPEN_POPUP_HWND.swap(null_mut(), Ordering::AcqRel);
        if !existing.is_null() {
            let _ = DestroyWindow(HWND(existing));
            return;
        }
        let hwnd = create_popup(percent, state);
        if !hwnd.0.is_null() {
            OPEN_POPUP_HWND.store(hwnd.0, Ordering::Release);
        }
    }
}

unsafe fn create_popup(percent: Option<u8>, state: PopupState) -> HWND {
    let hmodule = GetModuleHandleW(None).unwrap_or_default();
    let hinst = HINSTANCE(hmodule.0);

    if !CLASS_REGISTERED.load(Ordering::Acquire) {
        let cursor = LoadCursorW(None, IDC_ARROW).unwrap_or(HCURSOR(null_mut()));
        let wc = WNDCLASSW {
            style: WNDCLASS_STYLES(0),
            lpfnWndProc: Some(wnd_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: hinst,
            hIcon: HICON(null_mut()),
            hCursor: cursor,
            hbrBackground: HBRUSH(null_mut()),
            lpszMenuName: PCWSTR::null(),
            lpszClassName: w!("ViperTrayPopup"),
        };
        let _ = RegisterClassW(&wc);
        CLASS_REGISTERED.store(true, Ordering::Release);
    }

    // Box up the display data; pointer goes through CREATESTRUCT::lpCreateParams,
    // we stash it via GWLP_USERDATA in WM_NCCREATE, free in WM_NCDESTROY.
    let boxed = Box::new(PopupData { percent, state });
    let create_param: *mut PopupData = Box::into_raw(boxed);

    // Position: centered above cursor, clamped to the relevant monitor's work area.
    let mut pt = POINT::default();
    let _ = GetCursorPos(&mut pt);
    let (x, y) = clamp_to_work_area(pt, POPUP_W, POPUP_H);

    let hwnd = CreateWindowExW(
        WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
        w!("ViperTrayPopup"),
        w!(""),
        WS_POPUP | WS_BORDER,
        x,
        y,
        POPUP_W,
        POPUP_H,
        HWND(null_mut()),
        HMENU(null_mut()),
        hinst,
        Some(create_param as *const c_void),
    )
    .unwrap_or(HWND(null_mut()));

    if hwnd.0.is_null() {
        // Window creation failed — reclaim the boxed text so we don't leak.
        let _ = Box::from_raw(create_param);
        return HWND(null_mut());
    }

    let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
    let _ = UpdateWindow(hwnd);

    // Capture the mouse so clicks anywhere — including outside the popup — come to us.
    SetCapture(hwnd);

    // Auto-dismiss timer: 5 seconds.
    SetTimer(hwnd, AUTO_DISMISS_TIMER_ID, 5000, None);

    hwnd
}

const AUTO_DISMISS_TIMER_ID: usize = 1;

unsafe fn clamp_to_work_area(cursor: POINT, win_w: i32, win_h: i32) -> (i32, i32) {
    // Pick the monitor the cursor is on; fall back gracefully if MonitorFromPoint fails.
    let mon = MonitorFromPoint(cursor, MONITOR_DEFAULTTONEAREST);
    let mut info: MONITORINFO = std::mem::zeroed();
    info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
    let work = if GetMonitorInfoW(mon, &mut info).as_bool() {
        info.rcWork
    } else {
        RECT {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1080,
        }
    };

    // Default position: centered above cursor.
    let mut x = cursor.x - win_w / 2;
    let mut y = cursor.y - win_h - 12;

    // Horizontal clamp.
    if x + win_w > work.right {
        x = work.right - win_w - 4;
    }
    if x < work.left {
        x = work.left + 4;
    }

    // Vertical: if it would spill above the work area, flip below the cursor.
    if y < work.top {
        y = cursor.y + 16;
    }
    if y + win_h > work.bottom {
        y = work.bottom - win_h - 4;
    }

    (x, y)
}

unsafe fn create_font(height: i32, weight: i32) -> HFONT {
    CreateFontW(
        height,
        0,
        0,
        0,
        weight,
        0,
        0,
        0,
        DEFAULT_CHARSET.0 as u32,
        OUT_DEFAULT_PRECIS.0 as u32,
        CLIP_DEFAULT_PRECIS.0 as u32,
        CLEARTYPE_QUALITY.0 as u32,
        DEFAULT_PITCH.0 as u32 | (FF_DONTCARE.0 as u32) << 4,
        w!("Segoe UI"),
    )
}

unsafe fn fill_rect_color(hdc: HDC, left: i32, top: i32, right: i32, bottom: i32, color: COLORREF) {
    let rect = RECT {
        left,
        top,
        right,
        bottom,
    };
    let brush = CreateSolidBrush(color);
    let _ = FillRect(hdc, &rect, brush);
    let _ = DeleteObject(brush);
}

fn battery_color(percent: Option<u8>, state: PopupState) -> COLORREF {
    match state {
        PopupState::Checking => COLORREF(0x00A0A0A0),
        PopupState::Offline => COLORREF(0x005A5A5A),
        PopupState::Retrying => COLORREF(0x0000A5FF),
        PopupState::Connected => match percent.unwrap_or(0) {
            0..=19 => COLORREF(0x003C3CFF),
            20..=59 => COLORREF(0x0000A5FF),
            _ => COLORREF(0x005ADC46),
        },
    }
}

fn status_text(state: PopupState) -> &'static str {
    match state {
        PopupState::Checking => "checking",
        PopupState::Connected => "connected",
        PopupState::Retrying => "retrying",
        PopupState::Offline => "offline",
    }
}

unsafe fn draw_text_line(
    hdc: HDC,
    text: &str,
    rect: &mut RECT,
    font: HFONT,
    color: COLORREF,
    flags: DRAW_TEXT_FORMAT,
) {
    let old_font = SelectObject(hdc, font);
    SetBkMode(hdc, TRANSPARENT);
    SetTextColor(hdc, color);
    let mut buf: Vec<u16> = text.encode_utf16().collect();
    let _ = DrawTextW(hdc, &mut buf, rect, flags);
    SelectObject(hdc, old_font);
}

unsafe fn draw_battery_bar(hdc: HDC, percent: Option<u8>, state: PopupState) {
    let x = 16;
    let y = 48;
    let w = 154;
    let h = 10;
    let color = battery_color(percent, state);

    // Outer shell + terminal.
    fill_rect_color(hdc, x, y, x + w, y + h, COLORREF(0x00707070));
    fill_rect_color(
        hdc,
        x + w + 1,
        y + 3,
        x + w + 5,
        y + h - 3,
        COLORREF(0x00707070),
    );

    // Inner track and fill.
    fill_rect_color(
        hdc,
        x + 2,
        y + 2,
        x + w - 2,
        y + h - 2,
        COLORREF(0x002F2F2F),
    );
    if let Some(p) = percent {
        let fill_w = ((w - 4) * p.min(100) as i32) / 100;
        if fill_w > 0 {
            fill_rect_color(hdc, x + 2, y + 2, x + 2 + fill_w, y + h - 2, color);
        }
    }
}

unsafe fn dismiss(hwnd: HWND) {
    // Release capture first so it doesn't leak to whichever window becomes foreground.
    let _ = ReleaseCapture();
    let _ = KillTimer(hwnd, AUTO_DISMISS_TIMER_ID);
    OPEN_POPUP_HWND
        .compare_exchange(hwnd.0, null_mut(), Ordering::AcqRel, Ordering::Acquire)
        .ok();
    let _ = DestroyWindow(hwnd);
}

unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        // WM_NCCREATE is the earliest reliable hook — earlier than WM_CREATE.
        // We pull our boxed text pointer out of CREATESTRUCT and stash it via GWLP_USERDATA.
        WM_NCCREATE => {
            let cs = lp.0 as *const CREATESTRUCTW;
            if !cs.is_null() {
                let p = (*cs).lpCreateParams as isize;
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, p);
            }
            DefWindowProcW(hwnd, msg, wp, lp)
        }

        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let hdc = BeginPaint(hwnd, &mut ps);

            let mut rc = RECT::default();
            let _ = GetClientRect(hwnd, &mut rc);

            // Background fill: dark grey
            let bg = CreateSolidBrush(COLORREF(0x00282828));
            let _ = FillRect(hdc, &rc, bg);
            let _ = DeleteObject(bg);

            // Pull the boxed display data out of GWLP_USERDATA. We only read it.
            let p = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const PopupData;
            if !p.is_null() {
                let data: &PopupData = &*p;
                let title_font = create_font(-27, FW_BOLD.0 as i32);
                let meta_font = create_font(-13, FW_SEMIBOLD.0 as i32);
                let percent_text = data
                    .percent
                    .map(|p| format!("{p}%"))
                    .unwrap_or_else(|| "--".into());

                let mut pct_rc = RECT {
                    left: 14,
                    top: 8,
                    right: 104,
                    bottom: 42,
                };
                draw_text_line(
                    hdc,
                    &percent_text,
                    &mut pct_rc,
                    title_font,
                    COLORREF(0x00FFFFFF),
                    DT_LEFT | DT_VCENTER | DT_SINGLELINE,
                );

                let mut status_rc = RECT {
                    left: 108,
                    top: 15,
                    right: 180,
                    bottom: 36,
                };
                draw_text_line(
                    hdc,
                    status_text(data.state),
                    &mut status_rc,
                    meta_font,
                    battery_color(data.percent, data.state),
                    DT_RIGHT | DT_VCENTER | DT_SINGLELINE,
                );

                draw_battery_bar(hdc, data.percent, data.state);

                let _ = DeleteObject(title_font);
                let _ = DeleteObject(meta_font);
            }

            let _ = EndPaint(hwnd, &ps);
            LRESULT(0)
        }

        // Clicks anywhere — inside or outside the popup — dismiss. Capture made it possible.
        WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN | WM_NCLBUTTONDOWN | WM_NCRBUTTONDOWN
        | WM_NCMBUTTONDOWN => {
            dismiss(hwnd);
            LRESULT(0)
        }

        // Auto-dismiss after the timer fires (set to 5s in create_popup).
        WM_TIMER => {
            if wp.0 == AUTO_DISMISS_TIMER_ID {
                dismiss(hwnd);
            }
            LRESULT(0)
        }

        // If something else steals capture (system dialog, alt-tab, another popup), bail.
        WM_CAPTURECHANGED => {
            // Only dismiss if capture moved to a different window than ours.
            if HWND(lp.0 as *mut c_void) != hwnd {
                OPEN_POPUP_HWND
                    .compare_exchange(hwnd.0, null_mut(), Ordering::AcqRel, Ordering::Acquire)
                    .ok();
                let _ = DestroyWindow(hwnd);
            }
            LRESULT(0)
        }

        WM_NCDESTROY => {
            // Reclaim the boxed display data we stashed in WM_NCCREATE.
            let p = GetWindowLongPtrW(hwnd, GWLP_USERDATA);
            if p != 0 {
                let _ = Box::from_raw(p as *mut PopupData);
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            }
            OPEN_POPUP_HWND
                .compare_exchange(hwnd.0, null_mut(), Ordering::AcqRel, Ordering::Acquire)
                .ok();
            DefWindowProcW(hwnd, msg, wp, lp)
        }

        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}
