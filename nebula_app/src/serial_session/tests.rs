use super::*;

#[test]
fn bare_port_defaults_to_115200_8n1() {
    let config: SerialConfig = "serial:COM3".parse().unwrap();
    assert_eq!(
        config,
        SerialConfig {
            port: "COM3".into(),
            baud: 115_200,
            data_bits: 8,
            parity: Parity::None,
            stop_bits: 1,
        }
    );
}

#[test]
fn baud_and_frame_override_defaults() {
    let config: SerialConfig = "serial:/dev/ttyUSB0,9600,7e2".parse().unwrap();
    assert_eq!(
        config,
        SerialConfig {
            port: "/dev/ttyUSB0".into(),
            baud: 9600,
            data_bits: 7,
            parity: Parity::Even,
            stop_bits: 2,
        }
    );
}

#[test]
fn invalid_destinations_are_rejected() {
    for destination in [
        "COM3",
        "serial:",
        "serial:COM3,fast",
        "serial:COM3,9600,9N1",
        "serial:COM3,9600,8M1",
        "serial:COM3,9600,8N1,x",
    ] {
        assert!(destination.parse::<SerialConfig>().is_err(), "{destination}");
    }
}

#[test]
fn display_name_drops_options() {
    assert_eq!(display_name("serial:COM3,9600"), "COM3");
}
