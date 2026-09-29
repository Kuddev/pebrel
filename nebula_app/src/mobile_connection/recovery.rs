//! 进程级 LAN 恢复：网卡枚举和监听重建留在后台，不依赖设置页是否打开。

use super::*;
use std::time::Duration;

pub(super) fn available_address(saved: Option<IpAddr>, available: &[LanAddress]) -> Option<IpAddr> {
    saved
        .filter(|address| available.iter().any(|entry| entry.address == *address))
        .or_else(|| available.first().map(|entry| entry.address))
}

fn repair_lan() -> Result<(), Failure> {
    // 用户设置事务优先；监测不递增代际，也不拿旧偏好覆盖已经提交的新设置。
    let Ok(_operation) = OPERATIONS.try_lock() else { return Ok(()) };
    let generation = GENERATION.load(Ordering::Acquire);
    initialize()?;
    let (mut preferences, healthy) = {
        let manager = manager().lock().map_err(|_| Failure::Connection)?;
        let healthy = manager.lan.as_ref().is_some_and(|active| {
            !matches!(
                *active.handle.status.lock().unwrap_or_else(|error| error.into_inner()),
                Status::Failed | Status::Stopped
            )
        });
        (manager.configuration.preferences.clone(), healthy)
    };
    if !route_enabled(&preferences, Mode::Lan) {
        return Ok(());
    }
    let available = addresses().map_err(|_| Failure::Address)?;
    let address = available_address(preferences.address, &available).ok_or(Failure::Address)?;
    if healthy && preferences.address == Some(address) {
        return Ok(());
    }
    preferences.address = Some(address);
    apply_locked(generation, preferences, None).map(|_| ())
}

pub(super) fn start() {
    static STARTED: AtomicBool = AtomicBool::new(false);
    if STARTED.swap(true, Ordering::AcqRel) {
        return;
    }
    let generation = begin();
    let started =
        std::thread::Builder::new().name("pebrel-mobile-recovery".into()).spawn(move || {
            let initial = initialize().and_then(|_| {
                let preferences = manager()
                    .lock()
                    .map_err(|_| Failure::Connection)?
                    .configuration
                    .preferences
                    .clone();
                if preferences.enabled {
                    apply(generation, preferences, None).map(|_| ())
                } else {
                    Ok(())
                }
            });
            if let Err(error) = initial {
                log::warn!("mobile startup: {error:?}");
            }
            let mut previous_failure = initial.err();
            loop {
                // 只枚举本机网卡，不做探测连接；禁用 LAN 时跳过枚举，不占用 UI 帧预算。
                std::thread::sleep(Duration::from_secs(5));
                let failure = repair_lan().err();
                if failure != previous_failure {
                    if let Some(error) = failure.filter(|error| *error != Failure::Cancelled) {
                        log::warn!("mobile LAN recovery: {error:?}");
                    }
                    previous_failure = failure;
                }
            }
        });
    if let Err(error) = started {
        STARTED.store(false, Ordering::Release);
        log::warn!("mobile recovery thread: {}", error.kind());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn address(value: &str) -> LanAddress {
        LanAddress { address: value.parse().unwrap(), name: "fixture".into(), preferred: true }
    }

    #[test]
    fn preserves_the_selected_interface_while_it_exists() {
        let available = [address("192.168.1.7"), address("100.64.0.8")];
        assert_eq!(
            available_address(Some(available[1].address), &available),
            Some(available[1].address)
        );
    }

    #[test]
    fn an_absent_interface_uses_the_current_order_without_inventing_an_address() {
        let saved = Some("192.168.0.7".parse().unwrap());
        let available = [address("192.168.1.7")];
        assert_eq!(available_address(saved, &available), Some(available[0].address));
        assert_eq!(available_address(saved, &[]), None);
        assert_eq!(available_address(None, &available), Some(available[0].address));
    }
}
