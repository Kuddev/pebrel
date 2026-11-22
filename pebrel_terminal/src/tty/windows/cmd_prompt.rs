use crate::tty::Options;

const MARK: &str = "\x1b]1337;SetUserVar=pebrel_cmd_prompt=MQ==\x07";
const START: &str = "\x1b]133;A\x07";
const END: &str = "\x1b]133;B\x07\x1b]1337;SetUserVar=pebrel_cmd_prompt=MQ==\x07";

pub(super) fn prepare(config: &Options) -> Options {
    let mut prepared = config.clone();
    let Some(shell) = &config.shell else { return prepared };
    let name = shell.program().rsplit(['/', '\\']).next().unwrap_or_default();
    if !name.eq_ignore_ascii_case("cmd") && !name.eq_ignore_ascii_case("cmd.exe") {
        return prepared;
    }

    let prompt = config
        .env
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case("PROMPT"))
        .map(|(_, value)| value.clone())
        .or_else(|| (!config.env_is_complete).then(|| std::env::var("PROMPT").ok()).flatten())
        .unwrap_or_else(|| "$P$G".into());
    prepared.env.retain(|key, _| !key.eq_ignore_ascii_case("PROMPT"));
    // 只改子进程的环境副本；嵌套启动或重复准备不能不断叠加标记。
    let prompt = if prompt.starts_with(START) && prompt.ends_with(END) {
        prompt
    } else {
        let prompt = prompt.strip_prefix(MARK).unwrap_or(&prompt);
        format!("{START}{prompt}{END}")
    };
    prepared.env.insert("PROMPT".into(), prompt);
    prepared
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tty::Shell;

    fn cmd() -> Options {
        Options {
            shell: Some(Shell::new("C:\\Windows\\System32\\CMD.EXE".into(), vec!["/d".into()])),
            env_is_complete: true,
            ..Options::default()
        }
    }

    #[test]
    fn native_prompt_preserves_custom_text_without_mutating_caller() {
        let mut config = cmd();
        config.env.insert("Prompt".into(), "[$P]$_$G".into());
        let prepared = prepare(&config);
        assert_eq!(prepared.env.get("PROMPT").unwrap(), &format!("{START}[$P]$_$G{END}"));
        assert!(!prepared.env.contains_key("Prompt"));
        assert_eq!(config.env.get("Prompt").unwrap(), "[$P]$_$G");
    }

    #[test]
    fn native_prompt_defaults_and_is_idempotent() {
        let prepared = prepare(&cmd());
        assert_eq!(prepared.env.get("PROMPT").unwrap(), &format!("{START}$P$G{END}"));
        assert_eq!(prepare(&prepared), prepared);
    }

    #[test]
    fn native_prompt_does_not_instrument_other_shells() {
        for name in ["powershell.exe", "pwsh", "wsl.exe", "mycmd.exe"] {
            let mut config = cmd();
            config.shell = Some(Shell::new(name.into(), vec![]));
            assert_eq!(prepare(&config), config);
        }
        let mut config = cmd();
        config.shell = None;
        assert_eq!(prepare(&config), config);
    }

    #[test]
    fn native_prompt_preserves_explicit_empty_prompt() {
        let mut config = cmd();
        config.env.insert("PROMPT".into(), String::new());
        assert_eq!(prepare(&config).env.get("PROMPT").unwrap(), &format!("{START}{END}"));
    }

    #[test]
    fn native_prompt_upgrades_a_prefix_only_marker_without_duplicating_it() {
        let mut config = cmd();
        config.env.insert("PROMPT".into(), format!("{MARK}[$P]$S"));
        let prepared = prepare(&config);
        assert_eq!(prepared.env.get("PROMPT").unwrap(), &format!("{START}[$P]$S{END}"));
        assert_eq!(prepare(&prepared), prepared);
    }
}
