use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[cfg(target_os = "windows")]
use windows::Win32::System::Power::{
    SetSuspendState, RegisterPowerSettingNotification, GUID_MONITOR_POWER_ON,
    GUID_SYSTEM_AWAYMODE, GUID_SESSION_DISPLAY_STATUS,
};
#[cfg(target_os = "windows")]
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM, LRESULT, BOOL};
#[cfg(target_os = "windows")]
use windows::Win32::UI::WindowsAndMessaging::{
    DefWindowProcW, WNDCLASSW, WS_OVERLAPPEDWINDOW, CW_USEDEFAULT,
    CreateWindowExW, RegisterClassW, DestroyWindow,
    WM_POWERBROADCAST, PBT_APMRESUMEAUTOMATIC, PBT_APMRESUMESUSPEND,
    PBT_APMSUSPEND, PBT_APMPOWERSTATUSCHANGE,
    WNDPROC, CS_HREDRAW, CS_VREDRAW, WINDOW_STYLE,
};
#[cfg(target_os = "windows")]
use windows::core::PCWSTR;

pub enum PowerEvent {
    Suspend,
    DisplayOff,
    Resume,
    DisplayOn,
}

pub struct PowerMonitor {
    running: Arc<AtomicBool>,
    suspended: Arc<AtomicBool>,
    display_off: Arc<AtomicBool>,
    #[cfg(target_os = "windows")]
    thread_handle: Option<std::thread::JoinHandle<()>>,
    #[cfg(target_os = "windows")]
    hwnd: Arc<std::sync::Mutex<Option<isize>>>,
}

impl PowerMonitor {
    pub fn new() -> Self {
        Self {
            running: Arc::new(AtomicBool::new(false)),
            suspended: Arc::new(AtomicBool::new(false)),
            display_off: Arc::new(AtomicBool::new(false)),
            #[cfg(target_os = "windows")]
            thread_handle: None,
            #[cfg(target_os = "windows")]
            hwnd: Arc::new(std::sync::Mutex::new(None)),
        }
    }

    pub fn start(&mut self, callback: Box<dyn Fn(PowerEvent) + Send + 'static>) {
        self.running.store(true, Ordering::SeqCst);
        let running = Arc::clone(&self.running);
        let suspended = Arc::clone(&self.suspended);
        let display_off = Arc::clone(&self.display_off);

        #[cfg(target_os = "windows")]
        {
            let hwnd_arc = Arc::clone(&self.hwnd);
            let thread = std::thread::spawn(move || {
                Self::windows_event_loop(running, suspended, display_off, callback, hwnd_arc);
            });
            self.thread_handle = Some(thread);
        }

        #[cfg(not(target_os = "windows"))]
        {
            let _ = callback;
            eprintln!("Power monitor: only Windows is currently supported, running without power event detection");
        }
    }

    pub fn stop(&mut self) {
        self.running.store(false, Ordering::SeqCst);

        #[cfg(target_os = "windows")]
        {
            if let Some(hwnd_val) = *self.hwnd.lock().unwrap() {
                unsafe {
                    let _ = PostMessageW(HWND(hwnd_val), WM_CLOSE, WPARAM(0), LPARAM(0));
                }
            }
            if let Some(handle) = self.thread_handle.take() {
                let _ = handle.join();
            }
        }
    }

    pub fn is_suspended(&self) -> bool {
        self.suspended.load(Ordering::SeqCst)
    }

    pub fn is_display_off(&self) -> bool {
        self.display_off.load(Ordering::SeqCst)
    }

    pub fn should_pause(&self) -> bool {
        self.suspended.load(Ordering::SeqCst) || self.display_off.load(Ordering::SeqCst)
    }

    pub fn auto_pause_reason(&self) -> Option<String> {
        if self.suspended.load(Ordering::SeqCst) {
            Some("system_suspend".to_string())
        } else if self.display_off.load(Ordering::SeqCst) {
            Some("display_off".to_string())
        } else {
            None
        }
    }

    #[cfg(target_os = "windows")]
    fn windows_event_loop(
        running: Arc<AtomicBool>,
        suspended: Arc<AtomicBool>,
        display_off: Arc<AtomicBool>,
        callback: Box<dyn Fn(PowerEvent) + Send + 'static>,
        hwnd_arc: Arc<std::sync::Mutex<Option<isize>>>,
    ) {
        use windows::Win32::UI::WindowsAndMessaging::{
            MSG, GetMessageW, TranslateMessage, DispatchMessageW, WM_CLOSE, WM_DESTROY,
            PostQuitMessage,
        };

        unsafe {
            let class_name: Vec<u16> = "PixelRecorderPowerMonitor\0".encode_utf16().collect();

            let wnd_class = WNDCLASSW {
                lpfnWndProc: Some(Self::power_window_proc),
                hInstance: windows::Win32::System::LibraryLoader::GetModuleHandleW(None).ok().map(|h| h.into()).unwrap_or_default(),
                lpszClassName: PCWSTR(class_name.as_ptr()),
                style: CS_HREDRAW | CS_VREDRAW,
                cbClsExtra: 0,
                cbWndExtra: 0,
                hIcon: None,
                hCursor: None,
                hbrBackground: None,
                lpszMenuName: PCWSTR::null(),
            };

            if RegisterClassW(&wnd_class).0 == 0 {
                eprintln!("Failed to register power monitor window class");
                return;
            }

            let hwnd = CreateWindowExW(
                windows::Win32::UI::WindowsAndMessaging::WINDOW_EX_STYLE::default(),
                PCWSTR(class_name.as_ptr()),
                PCWSTR(class_name.as_ptr()),
                WINDOW_STYLE(0),
                0, 0, 0, 0,
                HWND::default(),
                None,
                wnd_class.hInstance,
                None,
            );

            if hwnd.is_invalid() {
                eprintln!("Failed to create power monitor window");
                return;
            }

            *hwnd_arc.lock().unwrap() = hwnd.0;

            let _hmon = RegisterPowerSettingNotification(
                hwnd,
                &GUID_MONITOR_POWER_ON,
                0,
            );
            let _hsession = RegisterPowerSettingNotification(
                hwnd,
                &GUID_SESSION_DISPLAY_STATUS,
                0,
            );
            let _haway = RegisterPowerSettingNotification(
                hwnd,
                &GUID_SYSTEM_AWAYMODE,
                0,
            );

            let ctx = PowerMonitorContext {
                running,
                suspended,
                display_off,
                callback,
            };
            let ctx_ptr = Box::into_raw(Box::new(ctx)) as isize;
            windows::Win32::UI::WindowsAndMessaging::SetWindowLongPtrW(
                hwnd,
                windows::Win32::UI::WindowsAndMessaging::GWLP_USERDATA,
                ctx_ptr,
            );

            let mut msg = MSG::default();
            while running.load(Ordering::SeqCst) {
                let result = GetMessageW(&mut msg, HWND::default(), 0, 0);
                if result.0 <= 0 {
                    break;
                }
                if msg.message == WM_CLOSE || msg.message == WM_DESTROY {
                    PostQuitMessage(0);
                    break;
                }
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }

            let _ = DestroyWindow(hwnd);

            let ctx_ptr = windows::Win32::UI::WindowsAndMessaging::GetWindowLongPtrW(
                hwnd,
                windows::Win32::UI::WindowsAndMessaging::GWLP_USERDATA,
            );
            if ctx_ptr != 0 {
                let _ = Box::from_raw(ctx_ptr as *mut PowerMonitorContext);
            }
        }
    }

    #[cfg(target_os = "windows")]
    unsafe extern "system" fn power_window_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        if msg == WM_POWERBROADCAST {
            let ctx_ptr = windows::Win32::UI::WindowsAndMessaging::GetWindowLongPtrW(
                hwnd,
                windows::Win32::UI::WindowsAndMessaging::GWLP_USERDATA,
            );
            if ctx_ptr != 0 {
                let ctx = &*(ctx_ptr as *const PowerMonitorContext);

                match wparam.0 as u32 {
                    0x0004 | 0x000A => {
                        ctx.suspended.store(true, Ordering::SeqCst);
                        (ctx.callback)(PowerEvent::Suspend);
                    }
                    0x0007 | 0x0008 => {
                        ctx.suspended.store(false, Ordering::SeqCst);
                        (ctx.callback)(PowerEvent::Resume);
                    }
                    0x0009 => {
                        let power_setting = lparam.0 as *const windows::Win32::System::Power::POWERBROADCAST_SETTING;
                        if !power_setting.is_null() {
                            let data = (*power_setting).Data;
                            let is_on = data[0] != 0;
                            if is_on {
                                ctx.display_off.store(false, Ordering::SeqCst);
                                (ctx.callback)(PowerEvent::DisplayOn);
                            } else {
                                ctx.display_off.store(true, Ordering::SeqCst);
                                (ctx.callback)(PowerEvent::DisplayOff);
                            }
                        }
                    }
                    _ => {}
                }
            }
            return LRESULT(1);
        }

        DefWindowProcW(hwnd, msg, wparam, lparam)
    }
}

struct PowerMonitorContext {
    running: Arc<AtomicBool>,
    suspended: Arc<AtomicBool>,
    display_off: Arc<AtomicBool>,
    callback: Box<dyn Fn(PowerEvent) + Send + 'static>,
}

#[cfg(target_os = "windows")]
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_CLOSE};

impl Drop for PowerMonitor {
    fn drop(&mut self) {
        self.stop();
    }
}
