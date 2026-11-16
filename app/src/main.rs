//! The `openkumite` executable: the game is the library (lib.rs), which Android loads itself (`android_main`).
// (on Windows a window of its own: no console opens beside it when it starts from Explorer; one started from a terminal
// writes there, attached below. Debug builds keep theirs.)
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

fn main() -> impl std::process::Termination {
    #[cfg(windows)]
    attach_console();
    openkumite::main()
}

/// The terminal that started the game gets its output (`--help`, the log), unless that already goes somewhere.
#[cfg(windows)]
#[allow(unsafe_code)] // (the Win32 console FFI)
fn attach_console() {
    use windows_sys::Win32::System::Console::{AttachConsole, GetStdHandle, ATTACH_PARENT_PROCESS, STD_ERROR_HANDLE};
    // SAFETY: plain Win32 calls with constant arguments
    unsafe {
        if GetStdHandle(STD_ERROR_HANDLE).is_null() {
            AttachConsole(ATTACH_PARENT_PROCESS);
        }
    }
}
