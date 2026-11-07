--!strict
-- Editor types for `require '@pebrel'`, written by `pebrel config init` and
-- `pebrel config types`. Pebrel never executes this file; the running app
-- provides the real module. Regenerate it after upgrading Pebrel.

export type Platform = {
    os: string,
    arch: string,
    display_server: "wayland" | "x11" | "windows" | "unknown",
}

export type Config = {
    set_strict_mode: (self: Config, strict: boolean) -> (),
    [string]: any,
}

export type Pebrel = {
    config_file: string,
    config_dir: string,
    home_dir: string,
    executable_dir: string,
    version: string,
    target_triple: string,
    platform: Platform,
    array: <T>(values: { T }?) -> { T },
    config_builder: () -> Config,
    reload_configuration: () -> (),
    add_to_config_reload_watch_list: (path: string) -> (),
    log_error: (message: string) -> (),
    log_warn: (message: string) -> (),
    log_info: (message: string) -> (),
}

return (nil :: any) :: Pebrel
