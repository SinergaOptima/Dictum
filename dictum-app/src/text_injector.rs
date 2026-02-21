#[cfg(target_os = "windows")]
pub fn inject_text(text: &str) -> Result<(), String> {
    use std::{thread, time::Duration};

    let mut units: Vec<u16> = text.encode_utf16().collect();
    if units.is_empty() {
        return Ok(());
    }
    let tuning = inject_tuning(units.len());

    // Convert LF to CR so Enter behavior is consistent in classic Win32 apps.
    for unit in &mut units {
        if *unit == b'\n' as u16 {
            *unit = b'\r' as u16;
        }
    }

    wait_for_hotkey_modifiers_release();

    let method = inject_method();
    let target_proc = foreground_process_name().unwrap_or_default();
    let prefer_paste = matches!(method, InjectMethod::Paste)
        || (matches!(method, InjectMethod::Auto) && is_terminal_process(&target_proc));

    if prefer_paste {
        if let Err(e) = inject_via_clipboard_paste(text, &target_proc) {
            // Fall through to Unicode path; some apps reject simulated paste.
            tracing::debug!(error = %e, process = %target_proc, "clipboard paste injection failed; falling back to unicode");
        } else {
            thread::sleep(Duration::from_millis(tuning.clipboard_restore_delay_ms));
            return Ok(());
        }
    }

    if matches!(method, InjectMethod::Paste) {
        return Err("clipboard paste injection failed".into());
    }

    for chunk in units.chunks(tuning.chunk_units) {
        let mut last_err: Option<String> = None;
        for attempt in 0..tuning.retries {
            match send_unicode_chunk(chunk) {
                Ok(()) => {
                    last_err = None;
                    break;
                }
                Err(e) => {
                    last_err = Some(e);
                    if attempt + 1 < tuning.retries {
                        thread::sleep(Duration::from_millis(tuning.retry_delay_ms));
                    }
                }
            }
        }
        if let Some(e) = last_err {
            return Err(e);
        }
    }

    Ok(())
}

#[cfg(target_os = "windows")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InjectMethod {
    Auto,
    Unicode,
    Paste,
}

#[cfg(target_os = "windows")]
struct InjectTuning {
    chunk_units: usize,
    retries: usize,
    retry_delay_ms: u64,
    clipboard_restore_delay_ms: u64,
}

#[cfg(target_os = "windows")]
fn inject_tuning(total_units: usize) -> InjectTuning {
    let base_chunk = env_usize("DICTUM_INJECT_CHUNK_UNITS", 160, 48, 640);
    let adaptive_chunk = if total_units >= 4_000 {
        (base_chunk * 2).min(640)
    } else if total_units >= 1_600 {
        (base_chunk * 3 / 2).min(480)
    } else {
        base_chunk
    };

    InjectTuning {
        chunk_units: adaptive_chunk.max(1),
        retries: env_usize("DICTUM_INJECT_RETRIES", 2, 1, 5),
        retry_delay_ms: env_u64("DICTUM_INJECT_RETRY_DELAY_MS", 6, 1, 40),
        clipboard_restore_delay_ms: env_u64(
            "DICTUM_INJECT_CLIPBOARD_RESTORE_DELAY_MS",
            60,
            10,
            250,
        ),
    }
}

#[cfg(target_os = "windows")]
fn env_usize(key: &str, default_value: usize, min: usize, max: usize) -> usize {
    std::env::var(key)
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .map(|v| v.clamp(min, max))
        .unwrap_or(default_value)
}

#[cfg(target_os = "windows")]
fn env_u64(key: &str, default_value: u64, min: u64, max: u64) -> u64 {
    std::env::var(key)
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .map(|v| v.clamp(min, max))
        .unwrap_or(default_value)
}

#[cfg(target_os = "windows")]
fn inject_method() -> InjectMethod {
    match std::env::var("DICTUM_INJECT_METHOD")
        .ok()
        .map(|v| v.trim().to_ascii_lowercase())
        .as_deref()
    {
        Some("unicode") => InjectMethod::Unicode,
        Some("paste") => InjectMethod::Paste,
        _ => InjectMethod::Auto,
    }
}

#[cfg(target_os = "windows")]
fn is_terminal_process(process_name: &str) -> bool {
    matches!(
        process_name,
        "warp.exe"
            | "windowsterminal.exe"
            | "wezterm-gui.exe"
            | "alacritty.exe"
            | "cmd.exe"
            | "conhost.exe"
            | "powershell.exe"
            | "pwsh.exe"
            | "mintty.exe"
    )
}

#[cfg(target_os = "windows")]
fn wait_for_hotkey_modifiers_release() {
    use std::{thread, time::Duration};
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        GetAsyncKeyState, VK_CONTROL, VK_LMENU, VK_LSHIFT, VK_LWIN, VK_MENU, VK_RMENU, VK_RSHIFT,
        VK_RWIN, VK_SHIFT,
    };

    fn is_modifier_down(vk: u16) -> bool {
        // SAFETY: Win32 call reads global key state and has no Rust-side aliasing requirements.
        unsafe { (GetAsyncKeyState(vk as i32) as u16 & 0x8000) != 0 }
    }

    for _ in 0..7 {
        if !is_modifier_down(VK_CONTROL)
            && !is_modifier_down(VK_SHIFT)
            && !is_modifier_down(VK_MENU)
            && !is_modifier_down(VK_LSHIFT)
            && !is_modifier_down(VK_RSHIFT)
            && !is_modifier_down(VK_LMENU)
            && !is_modifier_down(VK_RMENU)
            && !is_modifier_down(VK_LWIN)
            && !is_modifier_down(VK_RWIN)
        {
            return;
        }
        thread::sleep(Duration::from_millis(3));
    }
}

#[cfg(target_os = "windows")]
fn inject_via_clipboard_paste(text: &str, target_proc: &str) -> Result<(), String> {
    use std::{thread, time::Duration};

    let previous = read_clipboard_unicode_text();
    set_clipboard_unicode_text(text)?;

    let paste_result = if target_proc == "warp.exe" {
        send_key_chord(&[vk_control(), vk_shift()], vk_v())
            .or_else(|_| send_key_chord(&[vk_control()], vk_v()))
    } else {
        send_key_chord(&[vk_control()], vk_v())
            .or_else(|_| send_key_chord(&[vk_control(), vk_shift()], vk_v()))
    };

    let restore_result = if let Some(prev) = previous {
        // Give the target app enough time to read clipboard before restore.
        thread::sleep(Duration::from_millis(45));
        set_clipboard_unicode_text(&prev)
    } else {
        Ok(())
    };

    paste_result?;
    restore_result?;
    Ok(())
}

#[cfg(target_os = "windows")]
fn read_clipboard_unicode_text() -> Option<String> {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::System::DataExchange::{
        CloseClipboard, GetClipboardData, IsClipboardFormatAvailable,
    };
    use windows_sys::Win32::System::Memory::{GlobalLock, GlobalUnlock};

    const CF_UNICODETEXT: u32 = 13;
    if !open_clipboard_with_retry(std::ptr::null_mut()) {
        return None;
    }

    let result = unsafe {
        if IsClipboardFormatAvailable(CF_UNICODETEXT) == 0 {
            None
        } else {
            let h = GetClipboardData(CF_UNICODETEXT);
            if h.is_null() {
                None
            } else {
                let ptr = GlobalLock(h as _) as *const u16;
                if ptr.is_null() {
                    None
                } else {
                    let mut len = 0usize;
                    while *ptr.add(len) != 0 {
                        len += 1;
                    }
                    let slice = std::slice::from_raw_parts(ptr, len);
                    let out = OsString::from_wide(slice).to_string_lossy().to_string();
                    let _ = GlobalUnlock(h as _);
                    Some(out)
                }
            }
        }
    };

    unsafe {
        CloseClipboard();
    }
    result
}

#[cfg(target_os = "windows")]
fn set_clipboard_unicode_text(text: &str) -> Result<(), String> {
    use windows_sys::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, SetClipboardData,
    };
    use windows_sys::Win32::System::Memory::{
        GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE,
    };

    const CF_UNICODETEXT: u32 = 13;
    let mut utf16: Vec<u16> = text.encode_utf16().collect();
    utf16.push(0);
    let bytes = utf16.len() * std::mem::size_of::<u16>();

    if !open_clipboard_with_retry(std::ptr::null_mut()) {
        return Err("OpenClipboard failed".into());
    }

    let result = unsafe {
        if EmptyClipboard() == 0 {
            Err("EmptyClipboard failed".to_string())
        } else {
            let hmem = GlobalAlloc(GMEM_MOVEABLE, bytes);
            if hmem.is_null() {
                Err("GlobalAlloc failed for clipboard text".to_string())
            } else {
                let dst = GlobalLock(hmem) as *mut u16;
                if dst.is_null() {
                    Err("GlobalLock failed for clipboard text".to_string())
                } else {
                    std::ptr::copy_nonoverlapping(utf16.as_ptr(), dst, utf16.len());
                    let _ = GlobalUnlock(hmem);
                    let set = SetClipboardData(CF_UNICODETEXT, hmem as *mut _);
                    if set.is_null() {
                        Err("SetClipboardData(CF_UNICODETEXT) failed".to_string())
                    } else {
                        Ok(())
                    }
                }
            }
        }
    };

    unsafe {
        CloseClipboard();
    }
    result
}

#[cfg(target_os = "windows")]
fn open_clipboard_with_retry(owner: windows_sys::Win32::Foundation::HWND) -> bool {
    use std::{thread, time::Duration};
    use windows_sys::Win32::System::DataExchange::OpenClipboard;
    for _ in 0..8 {
        let opened = unsafe { OpenClipboard(owner) != 0 };
        if opened {
            return true;
        }
        thread::sleep(Duration::from_millis(8));
    }
    false
}

#[cfg(target_os = "windows")]
fn foreground_process_name() -> Option<String> {
    use std::path::Path;
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowThreadProcessId,
    };

    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_null() {
            return None;
        }
        let mut pid = 0u32;
        let _ = GetWindowThreadProcessId(hwnd, &mut pid);
        if pid == 0 {
            return None;
        }
        let hproc: HANDLE = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if hproc.is_null() {
            return None;
        }
        let mut buf = vec![0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(hproc, PROCESS_NAME_WIN32, buf.as_mut_ptr(), &mut len);
        let _ = CloseHandle(hproc);
        if ok == 0 || len == 0 {
            return None;
        }
        let full = String::from_utf16_lossy(&buf[..len as usize]);
        let exe = Path::new(&full)
            .file_name()?
            .to_string_lossy()
            .to_ascii_lowercase();
        Some(exe)
    }
}

#[cfg(target_os = "windows")]
fn vk_control() -> u16 {
    windows_sys::Win32::UI::Input::KeyboardAndMouse::VK_CONTROL
}

#[cfg(target_os = "windows")]
fn vk_shift() -> u16 {
    windows_sys::Win32::UI::Input::KeyboardAndMouse::VK_SHIFT
}

#[cfg(target_os = "windows")]
fn vk_v() -> u16 {
    windows_sys::Win32::UI::Input::KeyboardAndMouse::VK_V
}

#[cfg(target_os = "windows")]
fn send_key_chord(modifiers: &[u16], key: u16) -> Result<(), String> {
    use std::mem::size_of;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP,
    };

    let mut inputs: Vec<INPUT> = Vec::with_capacity(modifiers.len() * 2 + 2);

    for &vk in modifiers {
        inputs.push(INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: vk,
                    wScan: 0,
                    dwFlags: 0,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        });
    }

    inputs.push(INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: key,
                wScan: 0,
                dwFlags: 0,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    });
    inputs.push(INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: key,
                wScan: 0,
                dwFlags: KEYEVENTF_KEYUP,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    });

    for &vk in modifiers.iter().rev() {
        inputs.push(INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: vk,
                    wScan: 0,
                    dwFlags: KEYEVENTF_KEYUP,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        });
    }

    let sent = unsafe {
        SendInput(
            inputs.len() as u32,
            inputs.as_ptr(),
            size_of::<INPUT>() as i32,
        )
    };
    if sent != inputs.len() as u32 {
        let win_err = std::io::Error::last_os_error();
        return Err(format!(
            "SendInput chord sent {sent}/{} events (os_error={win_err})",
            inputs.len()
        ));
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn send_unicode_chunk(chunk: &[u16]) -> Result<(), String> {
    use std::mem::size_of;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE,
    };

    let mut inputs: Vec<INPUT> = Vec::with_capacity(chunk.len() * 2);
    for &scan in chunk {
        inputs.push(INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: 0,
                    wScan: scan,
                    dwFlags: KEYEVENTF_UNICODE,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        });
        inputs.push(INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: 0,
                    wScan: scan,
                    dwFlags: KEYEVENTF_UNICODE | KEYEVENTF_KEYUP,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        });
    }

    // SAFETY: `inputs` points to initialized `INPUT` structs and lives
    // for the duration of the call.
    let sent = unsafe {
        SendInput(
            inputs.len() as u32,
            inputs.as_ptr(),
            size_of::<INPUT>() as i32,
        )
    };
    if sent != inputs.len() as u32 {
        let win_err = std::io::Error::last_os_error();
        return Err(format!(
            "SendInput sent {sent}/{} keyboard events (os_error={win_err})",
            inputs.len()
        ));
    }
    Ok(())
}

#[cfg(not(target_os = "windows"))]
pub fn inject_text(_text: &str) -> Result<(), String> {
    Ok(())
}
