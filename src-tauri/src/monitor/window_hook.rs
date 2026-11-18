use std::path::Path;
use windows::Win32::Foundation::{CloseHandle, HWND};
use windows::Win32::System::ProcessStatus::K32GetProcessImageFileNameW;
use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowExW, GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId,
};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ActiveWindowInfo {
    pub title: String,
    pub process_name: String,
    pub is_empty: bool,
    pub lookup_failed: bool,
}

impl Default for ActiveWindowInfo {
    fn default() -> Self {
        Self {
            title: String::new(),
            process_name: String::new(),
            is_empty: true,
            lookup_failed: false,
        }
    }
}

pub fn get_active_window_info() -> ActiveWindowInfo {
    let hwnd: HWND = unsafe { GetForegroundWindow() };
    if hwnd.0.is_null() {
        return ActiveWindowInfo::default();
    }

    // 1. Extract Window Title
    let mut title_buf = [0u16; 512];
    let len = unsafe { GetWindowTextW(hwnd, &mut title_buf) };
    let title = if len > 0 {
        String::from_utf16_lossy(&title_buf[..len as usize])
    } else {
        String::new()
    };

    // 2. Extract Process Executable Name
    let mut pid: u32 = 0;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    if pid == 0 {
        return ActiveWindowInfo {
            title,
            process_name: String::new(),
            is_empty: false,
            lookup_failed: true,
        };
    }

    let mut lookup_failed = false;
    let mut process_name = match unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) } {
        Ok(handle) => {
            let mut path_buf = [0u16; 512];
            let path_len = unsafe { K32GetProcessImageFileNameW(handle, &mut path_buf) };
            let _ = unsafe { CloseHandle(handle) };

            if path_len > 0 {
                let full_path = String::from_utf16_lossy(&path_buf[..path_len as usize]);
                Path::new(&full_path)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("")
                    .to_lowercase()
            } else {
                lookup_failed = true;
                String::new()
            }
        }
        Err(_) => {
            lookup_failed = true;
            String::new()
        }
    };

    // For UWP/Store apps, top-level window may belong to ApplicationFrameHost.exe.
    // Try to resolve child CoreWindow thread process if present.
    if process_name == "applicationframehost.exe" {
        if let Ok(child_hwnd) = unsafe {
            FindWindowExW(
                Some(hwnd),
                None,
                windows::core::w!("Windows.UI.Core.CoreWindow"),
                windows::core::PCWSTR::null(),
            )
        } {
            if !child_hwnd.0.is_null() {
                let mut child_pid: u32 = 0;
                unsafe { GetWindowThreadProcessId(child_hwnd, Some(&mut child_pid)) };
                if child_pid != 0 && child_pid != pid {
                    if let Ok(handle) = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, child_pid) } {
                        let mut path_buf = [0u16; 512];
                        let path_len = unsafe { K32GetProcessImageFileNameW(handle, &mut path_buf) };
                        let _ = unsafe { CloseHandle(handle) };
                        if path_len > 0 {
                            let full_path = String::from_utf16_lossy(&path_buf[..path_len as usize]);
                            if let Some(name) = Path::new(&full_path).file_name().and_then(|n| n.to_str()) {
                                process_name = name.to_lowercase();
                            }
                        }
                    }
                }
            }
        }
    }

    ActiveWindowInfo {
        title,
        process_name,
        is_empty: false,
        lookup_failed,
    }
}
