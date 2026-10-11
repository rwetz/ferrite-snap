//! Only captures HWNDs created by Snap's own gallery child. No desktop input.
use raw_window_handle::HasWindowHandle;
use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

pub fn ready(window: &gpui::Window) {
    static WRITTEN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    let Some(path) = std::env::var_os("FERRITE_SNAP_READY") else {
        return;
    };
    if WRITTEN.load(std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    if let Ok(raw_window_handle::RawWindowHandle::Win32(handle)) =
        HasWindowHandle::window_handle(window).map(|h| h.as_raw())
    {
        // Written during render; the parent waits another two seconds for paint.
        if std::fs::write(path, handle.hwnd.get().to_string()).is_ok() {
            WRITTEN.store(true, std::sync::atomic::Ordering::Relaxed);
        }
    }
}

pub fn capture(page: usize, scheme: &str, light: bool, destination: &Path) -> Result<(), String> {
    if page >= 8 || ferrite_design::schemes::by_key(scheme).is_none() {
        return Err("Invalid gallery selection".into());
    }
    let parent = destination.parent().ok_or("Missing capture directory")?;
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let ready_path = parent.join(format!("ready-{}-{}.txt", std::process::id(), page));
    let _ = std::fs::remove_file(&ready_path);
    let mut command =
        std::process::Command::new(std::env::current_exe().map_err(|e| e.to_string())?);
    command
        .arg("--gallery")
        .env("FERRITE_SNAP_READY", &ready_path)
        .env("FERRITE_SNAP_PAGE", page.to_string())
        .env("FERRITE_SCHEME", scheme)
        .env("FERRITE_APPEARANCE", if light { "light" } else { "dark" })
        .env("FERRITE_FPS", "30")
        .env_remove("FERRITE_UI_RECEIPT");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = command.spawn().map_err(|e| e.to_string())?;
    let result = (|| {
        let deadline = Instant::now() + Duration::from_secs(30);
        let hwnd = loop {
            if let Ok(text) = std::fs::read_to_string(&ready_path)
                && let Ok(handle) = text.trim().parse::<isize>()
            {
                break handle;
            }
            if child.try_wait().map_err(|e| e.to_string())?.is_some() {
                return Err("Gallery exited before painting".into());
            }
            if Instant::now() >= deadline {
                return Err("Gallery did not become ready within 30 seconds".into());
            }
            std::thread::sleep(Duration::from_millis(50));
        };
        std::thread::sleep(Duration::from_secs(2));
        #[cfg(windows)]
        {
            native::capture(hwnd, child.id(), destination)
        }
        #[cfg(not(windows))]
        {
            let _ = hwnd;
            Err("Native gallery capture currently requires Windows".into())
        }
    })();
    let _ = child.kill();
    let _ = child.wait();
    let _ = std::fs::remove_file(ready_path);
    result
}

pub fn root() -> PathBuf {
    std::env::var_os(if cfg!(windows) {
        "LOCALAPPDATA"
    } else {
        "HOME"
    })
    .map(PathBuf::from)
    .unwrap_or_else(std::env::temp_dir)
    .join("ferrite/snap")
}

#[cfg(windows)]
mod native {
    use super::*;
    use std::ffi::c_void;
    type Handle = *mut c_void;
    #[repr(C)]
    #[derive(Default)]
    struct Rect {
        left: i32,
        top: i32,
        right: i32,
        bottom: i32,
    }
    #[repr(C)]
    struct BitmapInfo {
        size: u32,
        width: i32,
        height: i32,
        planes: u16,
        bits: u16,
        compression: u32,
        image_size: u32,
        x: i32,
        y: i32,
        used: u32,
        important: u32,
        colors: [u32; 3],
    }
    #[link(name = "user32")]
    unsafe extern "system" {
        fn SetThreadDpiAwarenessContext(context: Handle) -> Handle;
        fn GetWindowRect(hwnd: Handle, rect: *mut Rect) -> i32;
        fn GetWindowThreadProcessId(hwnd: Handle, pid: *mut u32) -> u32;
        fn GetDC(hwnd: Handle) -> Handle;
        fn ReleaseDC(hwnd: Handle, dc: Handle) -> i32;
        fn PrintWindow(hwnd: Handle, dc: Handle, flags: u32) -> i32;
    }
    #[link(name = "gdi32")]
    unsafe extern "system" {
        fn CreateCompatibleDC(dc: Handle) -> Handle;
        fn CreateCompatibleBitmap(dc: Handle, width: i32, height: i32) -> Handle;
        fn SelectObject(dc: Handle, object: Handle) -> Handle;
        fn DeleteObject(object: Handle) -> i32;
        fn DeleteDC(dc: Handle) -> i32;
        fn GetDIBits(
            dc: Handle,
            bitmap: Handle,
            start: u32,
            lines: u32,
            data: *mut c_void,
            info: *mut BitmapInfo,
            usage: u32,
        ) -> i32;
    }
    pub fn capture(raw: isize, owner: u32, path: &Path) -> Result<(), String> {
        unsafe {
            let previous = SetThreadDpiAwarenessContext(-4isize as Handle);
            let result = (|| {
                let hwnd = raw as Handle;
                let mut pid = 0;
                GetWindowThreadProcessId(hwnd, &mut pid);
                if pid != owner {
                    return Err("Capture window is not owned by the gallery process".into());
                }
                let mut rect = Rect::default();
                if GetWindowRect(hwnd, &mut rect) == 0 {
                    return Err("Cannot measure capture window".into());
                }
                let (w, h) = (rect.right - rect.left, rect.bottom - rect.top);
                if !(1..=4096).contains(&w) || !(1..=4096).contains(&h) {
                    return Err("Unsupported capture size".into());
                }
                let dc = GetDC(hwnd);
                let memory = CreateCompatibleDC(dc);
                let bitmap = CreateCompatibleBitmap(dc, w, h);
                if dc.is_null() || memory.is_null() || bitmap.is_null() {
                    if !bitmap.is_null() {
                        DeleteObject(bitmap);
                    }
                    if !memory.is_null() {
                        DeleteDC(memory);
                    }
                    if !dc.is_null() {
                        ReleaseDC(hwnd, dc);
                    }
                    return Err("Cannot allocate capture bitmap".into());
                }
                let old = SelectObject(memory, bitmap);
                let painted = PrintWindow(hwnd, memory, 2);
                SelectObject(memory, old);
                let mut bytes = vec![0u8; w as usize * h as usize * 4];
                let mut info = BitmapInfo {
                    size: 40,
                    width: w,
                    height: -h,
                    planes: 1,
                    bits: 32,
                    compression: 0,
                    image_size: 0,
                    x: 0,
                    y: 0,
                    used: 0,
                    important: 0,
                    colors: [0; 3],
                };
                let lines = GetDIBits(
                    memory,
                    bitmap,
                    0,
                    h as u32,
                    bytes.as_mut_ptr().cast(),
                    &mut info,
                    0,
                );
                DeleteObject(bitmap);
                DeleteDC(memory);
                ReleaseDC(hwnd, dc);
                if painted == 0 || lines != h {
                    return Err("Windows did not return the gallery image".into());
                }
                for pixel in bytes.chunks_exact_mut(4) {
                    pixel.swap(0, 2);
                    pixel[3] = 255;
                }
                let image = image::RgbaImage::from_raw(w as u32, h as u32, bytes)
                    .ok_or("Invalid capture bitmap")?;
                // Blank output must not silently become a passing baseline.
                let first = image.get_pixel(0, 0);
                if image.pixels().all(|p| p == first) {
                    return Err("Gallery capture was blank".into());
                }
                image.save(path).map_err(|e| e.to_string())
            })();
            SetThreadDpiAwarenessContext(previous);
            result
        }
    }
}
