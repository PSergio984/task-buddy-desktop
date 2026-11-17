use std::path::Path;
use windows::Win32::Foundation::{CloseHandle, HWND};
use windows::Win32::System::ProcessStatus::K32GetProcessImageFileNameW;
use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ActiveWindowInfo {
    pub title: String,
    pub process_name: String,
    pub is_empty: bool,
}

impl Default for ActiveWindowInfo {
    fn default() -> Self {
        Self {
            title: String::new(),
            process_name: String::new(),
            is_empty: true,
        }
    }
}

pub fn get_active_window_info() -> ActiveWindowInfo {
    unsafe {
        let hwnd: HWND = GetForegroundWindow();
        if hwnd.0.is_null() {
            return ActiveWindowInfo::default();
        }

        // 1. Extract Window Title
        let mut title_buf = [0u16; 512];
        let len = GetWindowTextW(hwnd, &mut title_buf);
        let title = if len > 0 {
            String::from_utf16_lossy(&title_buf[..len as usize])
        } else {
            String::new()
        };

        // 2. Extract Process Executable Name
        let mut pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 {
            return ActiveWindowInfo {
                title,
                process_name: String::new(),
                is_empty: false,
            };
        }

        let process_name = match OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
            Ok(handle) => {
                let mut path_buf = [0u16; 512];
                let path_len = K32GetProcessImageFileNameW(handle, &mut path_buf);
                let _ = CloseHandle(handle);

                if path_len > 0 {
                    let full_path = String::from_utf16_lossy(&path_buf[..path_len as usize]);
                    Path::new(&full_path)
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("")
                        .to_lowercase()
                } else {
                    String::new()
                }
            }
            Err(_) => String::new(),
        };

        ActiveWindowInfo {
            title,
            process_name,
            is_empty: false,
        }
    }
}
