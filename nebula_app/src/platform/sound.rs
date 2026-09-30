//! 使用系统音频服务的命令入口；播放与回收都在有界后台任务中，失败仍保留视觉铃声。
use std::io::Write as _;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

static PLAYING: AtomicBool = AtomicBool::new(false);

pub(super) fn play() -> bool {
    let program = if cfg!(target_os = "macos") {
        ["/usr/bin/afplay", "", ""]
    } else {
        ["/usr/bin/pw-play", "/usr/bin/paplay", "/bin/paplay"]
    }
    .into_iter()
    .find(|path| !path.is_empty() && std::path::Path::new(path).is_file());
    let Some(program) = program else { return false };
    if PLAYING.swap(true, Ordering::AcqRel) {
        return true;
    }
    let result = std::thread::Builder::new().name("pebrel-bell".into()).spawn(move || {
        struct Reset;
        impl Drop for Reset {
            fn drop(&mut self) {
                PLAYING.store(false, Ordering::Release);
            }
        }
        let _reset = Reset;
        let mut command = Command::new(program);
        if cfg!(target_os = "macos") {
            command.arg("/System/Library/Sounds/Tink.aiff");
        } else if program.ends_with("pw-play") {
            command.arg("-");
        }
        let Ok(mut child) =
            command.stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null()).spawn()
        else {
            return;
        };
        if cfg!(target_os = "linux")
            && let Some(mut input) = child.stdin.take()
        {
            // 80ms 衰减正弦，避免依赖桌面主题是否安装了 bell.oga。
            let rate = 16000u32;
            let samples = 1280u32;
            let mut wav = Vec::with_capacity(44 + samples as usize * 2);
            wav.extend(b"RIFF");
            wav.extend((36 + samples * 2).to_le_bytes());
            wav.extend(b"WAVEfmt ");
            wav.extend(16u32.to_le_bytes());
            wav.extend(1u16.to_le_bytes());
            wav.extend(1u16.to_le_bytes());
            wav.extend(rate.to_le_bytes());
            wav.extend((rate * 2).to_le_bytes());
            wav.extend(2u16.to_le_bytes());
            wav.extend(16u16.to_le_bytes());
            wav.extend(b"data");
            wav.extend((samples * 2).to_le_bytes());
            for sample in 0..samples {
                let amplitude = (1.0 - sample as f32 / samples as f32) * 5000.0;
                let value = ((sample as f32 * std::f32::consts::TAU * 880.0 / rate as f32).sin()
                    * amplitude) as i16;
                wav.extend(value.to_le_bytes());
            }
            let _ = input.write_all(&wav);
        }
        drop(child.stdin.take());
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            match child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(20))
                },
                _ => {
                    let _ = child.kill();
                    let _ = child.wait();
                    break;
                },
            }
        }
    });
    if result.is_err() {
        PLAYING.store(false, Ordering::Release);
    }
    result.is_ok()
}
