//! 开机自启：HKCU\...\Run\OringinFix

use windows::core::{w, HSTRING};
use windows::Win32::Foundation::ERROR_FILE_NOT_FOUND;
use windows::Win32::System::Registry::{
    RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegSetValueExW, HKEY_CURRENT_USER, KEY_WRITE,
    REG_SZ,
};

const VALUE_NAME: windows::core::PCWSTR = w!("OringinFix");

fn run_key() -> windows::core::Result<windows::Win32::System::Registry::HKEY> {
    unsafe {
        let mut key = std::mem::zeroed();
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run"),
            None,
            KEY_WRITE,
            &mut key,
        )
        .ok()?;
        Ok(key)
    }
}

pub fn command_line(silent: bool) -> Result<String, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    if silent {
        Ok(format!("\"{}\" --silent", exe.display()))
    } else {
        Ok(format!("\"{}\"", exe.display()))
    }
}

pub fn is_enabled() -> bool {
    let Ok(exe) = std::env::current_exe() else {
        return false;
    };
    let Ok(key) = run_key() else {
        return false;
    };
    let mut buf = [0u16; 1024];
    let mut size = (buf.len() * 2) as u32;
    let mut ty = windows::Win32::System::Registry::REG_VALUE_TYPE::default();
    let ok = unsafe {
        windows::Win32::System::Registry::RegQueryValueExW(
            key,
            VALUE_NAME,
            None,
            Some(&mut ty),
            Some(buf.as_mut_ptr() as *mut u8),
            Some(&mut size),
        )
        .is_ok()
    };
    unsafe {
        let _ = RegCloseKey(key);
    }
    if !ok {
        return false;
    }
    let len = (size as usize / 2).saturating_sub(1);
    let stored = String::from_utf16_lossy(&buf[..len]);
    let path = exe.to_string_lossy();
    stored.contains(path.as_ref())
}

pub fn set_enabled(on: bool, silent: bool) -> Result<(), String> {
    let key = run_key().map_err(|e| e.to_string())?;
    let result = if on {
        let value = HSTRING::from(command_line(silent)?);
        let bytes = unsafe {
            std::slice::from_raw_parts(value.as_ptr() as *const u8, (value.len() + 1) * 2)
        };
        unsafe {
            RegSetValueExW(key, VALUE_NAME, None, REG_SZ, Some(bytes))
                .ok()
                .map_err(|e| e.to_string())
        }
    } else {
        unsafe {
            match RegDeleteValueW(key, VALUE_NAME).ok() {
                Ok(()) => Ok(()),
                Err(e) if e.code() == ERROR_FILE_NOT_FOUND.to_hresult() => Ok(()),
                Err(e) => Err(e.to_string()),
            }
        }
    };
    unsafe {
        let _ = RegCloseKey(key);
    }
    result
}
