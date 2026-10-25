//! A bounded, transient client-area snapshot for native theme transitions.

use gpui::Window;
use std::io;

pub(crate) struct ClientFrame {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) bgra: Vec<u8>,
}

pub(crate) fn supported() -> bool {
    cfg!(windows)
}

#[cfg(not(windows))]
pub(crate) fn capture(_window: &Window) -> io::Result<ClientFrame> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "Client-area theme snapshots are not supported on this platform",
    ))
}

#[cfg(windows)]
pub(crate) fn capture(window: &Window) -> io::Result<ClientFrame> {
    use std::ptr;
    use windows_sys::Win32::Foundation::{HWND, RECT};
    use windows_sys::Win32::Graphics::Gdi::{
        BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS,
        DeleteDC, DeleteObject, GdiFlush, GetDC, HBITMAP, HDC, HGDIOBJ, RGBQUAD, ReleaseDC,
        SelectObject,
    };
    use windows_sys::Win32::Storage::Xps::{PW_CLIENTONLY, PrintWindow};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetClientRect, IsIconic, PW_RENDERFULLCONTENT,
    };
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};

    struct Capture {
        window: HWND,
        source: HDC,
        memory: HDC,
        bitmap: HBITMAP,
        previous: HGDIOBJ,
    }

    impl Drop for Capture {
        fn drop(&mut self) {
            // SAFETY: every non-null handle is owned here; restore before deletion.
            unsafe {
                if !self.previous.is_null() {
                    SelectObject(self.memory, self.previous);
                }
                if !self.bitmap.is_null() {
                    DeleteObject(self.bitmap);
                }
                if !self.memory.is_null() {
                    DeleteDC(self.memory);
                }
                if !self.source.is_null() {
                    ReleaseDC(self.window, self.source);
                }
            }
        }
    }

    let handle = HasWindowHandle::window_handle(window).map_err(|error| {
        io::Error::other(format!("Could not access the client window: {error}"))
    })?;
    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        return Err(io::Error::other("Expected a Win32 client window"));
    };
    let hwnd = handle.hwnd.get() as HWND;
    let mut rect = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    // SAFETY: hwnd is borrowed from the live GPUI window; rect is writable.
    if unsafe { IsIconic(hwnd) != 0 || GetClientRect(hwnd, &mut rect) == 0 } {
        return Err(io::Error::other("The client window is not drawable"));
    }
    let width = u32::try_from(rect.right - rect.left)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let height = u32::try_from(rect.bottom - rect.top)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let bytes = u64::from(width) * u64::from(height) * 4;
    if width == 0 || height == 0 || width > 4096 || height > 4096 || bytes > 32 * 1024 * 1024 {
        return Err(io::Error::other("Client snapshot exceeds the 32 MiB transition budget"));
    }
    let mut capture = Capture {
        window: hwnd,
        source: ptr::null_mut(),
        memory: ptr::null_mut(),
        bitmap: ptr::null_mut(),
        previous: ptr::null_mut(),
    };
    let mut pixels = ptr::null_mut();
    let bitmap_info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width as i32,
            biHeight: -(height as i32),
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB,
            biSizeImage: bytes as u32,
            biXPelsPerMeter: 0,
            biYPelsPerMeter: 0,
            biClrUsed: 0,
            biClrImportant: 0,
        },
        bmiColors: [RGBQUAD { rgbBlue: 0, rgbGreen: 0, rgbRed: 0, rgbReserved: 0 }],
    };
    // SAFETY: handles and the top-down 32-bit DIB are owned by Capture; the
    // checked size describes the accessible pixel buffer until bitmap deletion.
    unsafe {
        capture.source = GetDC(hwnd);
        if capture.source.is_null() {
            return Err(io::Error::other("Could not obtain the client device context"));
        }
        capture.memory = CreateCompatibleDC(capture.source);
        if capture.memory.is_null() {
            return Err(io::Error::other("Could not create the snapshot device context"));
        }
        capture.bitmap = CreateDIBSection(
            capture.source,
            &bitmap_info,
            DIB_RGB_COLORS,
            &mut pixels,
            ptr::null_mut(),
            0,
        );
        if capture.bitmap.is_null() || pixels.is_null() {
            return Err(io::Error::other("Could not allocate the snapshot bitmap"));
        }
        capture.previous = SelectObject(capture.memory, capture.bitmap);
        if capture.previous.is_null() || capture.previous as isize == -1 {
            capture.previous = ptr::null_mut();
            return Err(io::Error::other("Could not select the snapshot bitmap"));
        }
        // GPU 合成内容不在窗口 DC 中；请求自有窗口的完整客户端画面，
        // 不改用桌面截图，避免把遮挡窗口或其他应用的像素带入主题过渡。
        if PrintWindow(hwnd, capture.memory, PW_CLIENTONLY | PW_RENDERFULLCONTENT) == 0 {
            return Err(io::Error::other("Could not capture the client pixels"));
        }
        if GdiFlush() == 0 {
            return Err(io::Error::other("Could not finish the client snapshot"));
        }
        let mut bgra = std::slice::from_raw_parts(pixels.cast::<u8>(), bytes as usize).to_vec();
        if bgra.chunks_exact(4).all(|pixel| pixel[..3] == [0, 0, 0]) {
            return Err(io::Error::other("Client capture returned an empty frame"));
        }
        for pixel in bgra.chunks_exact_mut(4) {
            pixel[3] = 255;
        }
        Ok(ClientFrame { width, height, bgra })
    }
}
