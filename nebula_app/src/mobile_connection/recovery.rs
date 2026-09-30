//! 进程级 LAN 恢复：网卡枚举和监听重建留在后台，不依赖设置页是否打开。

use super::*;
use std::time::Duration;

pub(crate) fn available_address(saved: Option<IpAddr>, available: &[LanAddress]) -> Option<IpAddr> {
    match saved {
        // 已选接口短暂消失时等待原地址，不能把其他网卡当成同一条连接的恢复。
        Some(address) => available.iter().any(|entry| entry.address == address).then_some(address),
        None => available.first().map(|entry| entry.address),
    }
}

pub(super) fn prepare_lan_address(preferences: &mut Preferences, available: &[LanAddress]) -> bool {
    if let Some(address) = available_address(preferences.address, available) {
        preferences.address = Some(address);
        true
    } else {
        false
    }
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
    let Some(address) = available_address(preferences.address, &available) else {
        // 丢弃绑定到已消失接口的监听，避免网卡回来后复用状态尚未报错的旧 socket。
        manager().lock().map_err(|_| Failure::Connection)?.lan.take();
        return Err(Failure::Address);
    };
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
    fn an_absent_selected_interface_waits_without_switching_to_another_interface() {
        let saved = Some("192.168.0.7".parse().unwrap());
        let available = [address("192.168.1.7")];
        assert_eq!(available_address(saved, &available), None);
        assert_eq!(available_address(saved, &[]), None);
        assert_eq!(available_address(None, &available), Some(available[0].address));
    }

    #[test]
    fn tailscale_disconnect_and_restart_keep_the_same_selected_address() {
        for selected in ["100.64.0.8", "fd7a:115c:a1e0::8"] {
            let selected = address(selected);
            let other = address("192.168.1.7");
            let mut configuration = preferences::Configuration::default();
            configuration.preferences.address = Some(selected.address);
            assert!(prepare_lan_address(
                &mut configuration.preferences,
                &[other.clone(), selected.clone()],
            ));
            assert!(!prepare_lan_address(&mut configuration.preferences, &[other.clone()]));
            assert_eq!(configuration.preferences.address, Some(selected.address));
            // 断网期间重启也必须记住原接口，不能因重新加载配置而切到 Wi-Fi。
            let saved = serde_json::to_vec(&configuration).unwrap();
            let mut restored: preferences::Configuration = serde_json::from_slice(&saved).unwrap();
            assert!(!prepare_lan_address(&mut restored.preferences, &[other.clone()]));
            assert_eq!(restored.preferences.address, Some(selected.address));
            assert!(prepare_lan_address(&mut restored.preferences, &[other, selected.clone()],));
            assert_eq!(restored.preferences.address, Some(selected.address));
        }
    }

    #[test]
    fn only_an_unconfigured_connection_automatically_selects_an_interface() {
        let mut preferences = Preferences::default();
        assert!(!prepare_lan_address(&mut preferences, &[]));
        assert_eq!(preferences.address, None);
        let available = [address("192.168.1.7")];
        assert!(prepare_lan_address(&mut preferences, &available));
        assert_eq!(preferences.address, Some(available[0].address));
        assert!(!prepare_lan_address(&mut preferences, &[]));
        assert_eq!(preferences.address, Some(available[0].address));
        // 用户明确改选另一个接口仍生效。
        preferences.address = Some("100.64.0.8".parse().unwrap());
        assert!(prepare_lan_address(&mut preferences, &[address("100.64.0.8")]));
        assert_eq!(preferences.address, Some("100.64.0.8".parse().unwrap()));
    }
}
