#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
mod desktop;

fn main() -> anyhow::Result<()> {
    #[cfg(windows)]
    {
        if std::env::args_os().nth(1).is_none() {
            return desktop::launch();
        }
        attach_cli_console();
    }
    camo_translator::cli::run()
}

#[cfg(windows)]
fn attach_cli_console() {
    use windows_sys::Win32::Foundation::{HANDLE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Console::{
        ATTACH_PARENT_PROCESS, AttachConsole, GetStdHandle, STD_ERROR_HANDLE, STD_OUTPUT_HANDLE,
    };

    let missing = |handle: HANDLE| handle == INVALID_HANDLE_VALUE || handle.is_null();
    // Keep inherited redirections intact. Legacy interactive invocations need a console.
    unsafe {
        if missing(GetStdHandle(STD_OUTPUT_HANDLE)) && missing(GetStdHandle(STD_ERROR_HANDLE)) {
            AttachConsole(ATTACH_PARENT_PROCESS);
        }
    }
}
