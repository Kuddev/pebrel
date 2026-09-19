use crate::tty::Options;

const MARK: &str = "\x1b]1337;SetUserVar=pebrel_cmd_prompt=MQ==\x07";

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
    let prompt = if prompt.starts_with(MARK) { prompt } else { format!("{MARK}{prompt}") };
    prepared.env.insert("PROMPT".into(), prompt);
    prepared
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tty::Shell;

    const MARK: &str = "\x1b]1337;SetUserVar=pebrel_cmd_prompt=MQ==\x07";

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
        assert_eq!(prepared.env.get("PROMPT").unwrap(), &format!("{MARK}[$P]$_$G"));
        assert!(!prepared.env.contains_key("Prompt"));
        assert_eq!(config.env.get("Prompt").unwrap(), "[$P]$_$G");
    }

    #[test]
    fn native_prompt_defaults_and_is_idempotent() {
        let prepared = prepare(&cmd());
        assert_eq!(prepared.env.get("PROMPT").unwrap(), &format!("{MARK}$P$G"));
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
        assert_eq!(prepare(&config).env.get("PROMPT").unwrap(), MARK);
    }
}
