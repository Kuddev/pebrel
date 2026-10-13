//! 打开并配置串口。读操作最多阻塞约 100 ms（Windows 为 20 ms，同步句柄上的挂起读会阻塞写）后返回 0 字节，供读线程检查停止标志。

use std::fs::{File, OpenOptions};
use std::io;

use crate::serial_session::{Parity, SerialConfig};

#[cfg(unix)]
pub(crate) fn open(config: &SerialConfig) -> io::Result<File> {
    use std::os::fd::AsRawFd;
    use std::os::unix::fs::OpenOptionsExt;

    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(libc::O_NOCTTY | libc::O_NONBLOCK)
        .open(&config.port)?;
    let fd = file.as_raw_fd();
    let check = |status: libc::c_int| {
        if status == 0 { Ok(()) } else { Err(io::Error::last_os_error()) }
    };
    let mut termios: libc::termios = unsafe { std::mem::zeroed() };
    check(unsafe { libc::tcgetattr(fd, &mut termios) })?;
    unsafe { libc::cfmakeraw(&mut termios) };
    termios.c_cflag &= !(libc::CSIZE | libc::PARENB | libc::PARODD | libc::CSTOPB | libc::CRTSCTS);
    termios.c_cflag |= libc::CLOCAL
        | libc::CREAD
        | match config.data_bits {
            5 => libc::CS5,
            6 => libc::CS6,
            7 => libc::CS7,
            _ => libc::CS8,
        };
    match config.parity {
        Parity::None => {},
        Parity::Even => termios.c_cflag |= libc::PARENB,
        Parity::Odd => termios.c_cflag |= libc::PARENB | libc::PARODD,
    }
    if config.stop_bits == 2 {
        termios.c_cflag |= libc::CSTOPB;
    }
    let speed = baud_constant(config.baud)?;
    check(unsafe { libc::cfsetispeed(&mut termios, speed) })?;
    check(unsafe { libc::cfsetospeed(&mut termios, speed) })?;
    termios.c_cc[libc::VMIN] = 0;
    termios.c_cc[libc::VTIME] = 1;
    check(unsafe { libc::tcsetattr(fd, libc::TCSANOW, &termios) })?;
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 {
        return Err(io::Error::last_os_error());
    }
    check(unsafe { libc::fcntl(fd, libc::F_SETFL, flags & !libc::O_NONBLOCK) })?;
    Ok(file)
}

#[cfg(unix)]
fn baud_constant(baud: u32) -> io::Result<libc::speed_t> {
    Ok(match baud {
        1200 => libc::B1200,
        2400 => libc::B2400,
        4800 => libc::B4800,
        9600 => libc::B9600,
        19_200 => libc::B19200,
        38_400 => libc::B38400,
        57_600 => libc::B57600,
        115_200 => libc::B115200,
        230_400 => libc::B230400,
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("unsupported baud rate: {baud}"),
            ));
        },
    })
}

#[cfg(windows)]
pub(crate) fn open(config: &SerialConfig) -> io::Result<File> {
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Devices::Communication::{
        COMMTIMEOUTS, DCB, GetCommState, SetCommState, SetCommTimeouts,
    };

    // COM10 及以上必须使用 `\\.\` 设备命名空间前缀。
    let path = if config.port.starts_with(r"\\") {
        config.port.clone()
    } else {
        format!(r"\\.\{}", config.port)
    };
    let file = OpenOptions::new().read(true).write(true).share_mode(0).open(path)?;
    let handle = file.as_raw_handle();
    let check = |status: i32| if status != 0 { Ok(()) } else { Err(io::Error::last_os_error()) };
    let mut dcb: DCB = unsafe { std::mem::zeroed() };
    dcb.DCBlength = std::mem::size_of::<DCB>() as u32;
    check(unsafe { GetCommState(handle, &mut dcb) })?;
    dcb.BaudRate = config.baud;
    dcb.ByteSize = config.data_bits;
    dcb.Parity = match config.parity {
        Parity::None => 0,
        Parity::Odd => 1,
        Parity::Even => 2,
    };
    dcb.StopBits = if config.stop_bits == 2 { 2 } else { 0 };
    // fBinary | fParity | fDtrControl=ENABLE | fRtsControl=ENABLE；其余流控位清零。
    dcb._bitfield = 1 | (u32::from(config.parity != Parity::None) << 1) | (1 << 4) | (1 << 12);
    check(unsafe { SetCommState(handle, &dcb) })?;
    let timeouts = COMMTIMEOUTS {
        ReadIntervalTimeout: u32::MAX,
        ReadTotalTimeoutMultiplier: 0,
        ReadTotalTimeoutConstant: 20,
        WriteTotalTimeoutMultiplier: 0,
        WriteTotalTimeoutConstant: 5000,
    };
    check(unsafe { SetCommTimeouts(handle, &timeouts) })?;
    Ok(file)
}
