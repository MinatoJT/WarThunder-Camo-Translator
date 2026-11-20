#![cfg(windows)]

use std::os::windows::process::CommandExt;
use std::process::{Command, Stdio};
use std::thread::sleep;
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::{HWND, LPARAM};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible, PostMessageW, WM_CLOSE,
};

struct Search {
    pid: u32,
    window: HWND,
}

unsafe extern "system" fn locate(window: HWND, data: LPARAM) -> i32 {
    let search = unsafe { &mut *(data as *mut Search) };
    let mut pid = 0;
    unsafe { GetWindowThreadProcessId(window, &mut pid) };
    if pid == search.pid && unsafe { IsWindowVisible(window) } != 0 {
        let mut title = [0u16; 128];
        let len = unsafe { GetWindowTextW(window, title.as_mut_ptr(), title.len() as i32) };
        if len > 0 && String::from_utf16_lossy(&title[..len as usize]) == "Camo Translator" {
            search.window = window;
            return 0;
        }
    }
    1
}

#[test]
#[ignore = "requires an interactive Windows desktop"]
fn launching_without_arguments_opens_main_window() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_camo-translator"))
        .creation_flags(0x08000000)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();

    let mut search = Search {
        pid: child.id(),
        window: std::ptr::null_mut(),
    };
    let deadline = Instant::now() + Duration::from_secs(12);
    let mut early_exit = None;
    while Instant::now() < deadline {
        unsafe { EnumWindows(Some(locate), (&mut search as *mut Search) as LPARAM) };
        if !search.window.is_null() {
            unsafe { PostMessageW(search.window, WM_CLOSE, 0, 0) };
            break;
        }
        early_exit = child.try_wait().unwrap();
        if early_exit.is_some() {
            break;
        }
        sleep(Duration::from_millis(100));
    }
    let close_deadline = Instant::now() + Duration::from_secs(3);
    while child.try_wait().unwrap().is_none() && Instant::now() < close_deadline {
        sleep(Duration::from_millis(50));
    }
    if child.try_wait().unwrap().is_none() {
        let _ = child.kill();
    }
    let status = child.wait().unwrap();
    assert!(
        !search.window.is_null(),
        "main window missing, process exited with {early_exit:?}"
    );
    assert!(
        status.success(),
        "main window did not close cleanly: {status}"
    );
}

#[test]
fn windows_launchers_use_the_expected_subsystems() {
    for (path, subsystem) in [
        (env!("CARGO_BIN_EXE_camo-translator"), 2),
        (env!("CARGO_BIN_EXE_camo-translator-cli"), 3),
    ] {
        let data = std::fs::read(path).unwrap();
        let header = u32::from_le_bytes(data[0x3c..0x40].try_into().unwrap()) as usize;
        assert_eq!(&data[header..header + 4], b"PE\0\0");
        let offset = header + 24 + 68;
        let actual = u16::from_le_bytes(data[offset..offset + 2].try_into().unwrap());
        assert_eq!(actual, subsystem, "{path}");
    }
}
