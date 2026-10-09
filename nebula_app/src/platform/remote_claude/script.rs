//! 服务器端执行的 POSIX sh 脚本：预检/下发与会话引导。
//!
//! 两段都由本机生成并填好值，服务器上不安装任何东西：
//!
//! 1. **预检/下发**（`ssh <host> exec /bin/sh -s`，脚本走 stdin）：检查 claude 是否
//!    可用、是否已登录，建镜像目录，写入本次连接的临时私钥，清理残留连接目录。
//!    私钥只出现在 stdin 里，不进入任何进程的命令行。
//! 2. **会话引导**（`ssh -tt -R … <host> <launcher>`）：写入回连配置，经反向隧道
//!    自检本机 sshd，然后在镜像目录里前台运行 claude；退出、挂断或看门狗判定
//!    本机失联时由 trap 删除临时目录。
//!
//! 登录 shell 可能是 bash/zsh/fish/csh，所以命令行只用各家解析一致的写法：
//! 一个 `exec /bin/sh -c '<单引号内无特殊字符>'`，脚本本体以 base64 传入。

use base64::Engine as _;

use super::prompt::sh_quote;

/// 预检/下发的远端命令：脚本从 stdin 读入。
pub(super) const PROVISION_COMMAND: &str = "exec /bin/sh -s";

/// 会话引导脚本用来报告失败的退出码；与 Claude Code 自身的退出码（0/1）错开。
pub(super) const EXIT_NOT_PROVISIONED: i32 = 91;
pub(super) const EXIT_PROBE_FAILED: i32 = 92;
pub(super) const EXIT_MIRROR_MISSING: i32 = 93;

pub(super) struct Provision<'a> {
    pub(super) run_id: &'a str,
    pub(super) relative: &'a str,
    pub(super) project_key: &'a str,
    pub(super) ports: &'a [u16],
    /// OpenSSH 格式的临时登录私钥（LF 换行）。
    pub(super) identity: &'a str,
}

pub(super) fn provision_script(input: &Provision<'_>) -> String {
    let ports = input.ports.iter().map(u16::to_string).collect::<Vec<_>>().join(" ");
    fill(
        PROVISION,
        &[
            ("RUN_ID", sh_quote(input.run_id)),
            ("REL", sh_quote(input.relative)),
            ("KEY", sh_quote(input.project_key)),
            ("PORTS", sh_quote(&ports)),
            ("MIRROR_ROOT", super::mirror::MIRROR_ROOT.to_owned()),
            ("IDENTITY", input.identity.trim_end().to_owned()),
        ],
    )
}

/// 会话引导脚本里由本机预先格式化好的提示行。带 `%s` 的是 printf 模板，由脚本
/// 填入只有服务器才知道的值；其余是整行文本。
pub(super) struct SessionTexts {
    pub(super) tunnel: String,
    /// 模板：`%s` = 自检得到的 PowerShell 版本与用户名。
    pub(super) probe: String,
    pub(super) session: String,
    pub(super) not_provisioned: String,
    /// 模板：`%s` = 自检失败时 ssh 的最后几行输出。
    pub(super) probe_failed: String,
    pub(super) mirror_missing: String,
}

pub(super) struct Session<'a> {
    pub(super) run_id: &'a str,
    pub(super) project_key: &'a str,
    pub(super) remote_port: u16,
    /// 用户选中的 SSH 主机（别名或 `user@host`）：阶段帧里回传给宿主画卡片。
    pub(super) host: &'a str,
    pub(super) windows_user: &'a str,
    /// `ssh-ed25519 AAAA…`，本次临时 sshd 的主机公钥。
    pub(super) host_key: &'a str,
    pub(super) mirror: &'a str,
    /// 在 Windows 上执行、输出 `PEBREL_SSH_READY <detail>` 的自检命令行。
    pub(super) probe: &'a str,
    pub(super) prompt: &'a str,
    pub(super) claude_args: &'a [String],
    pub(super) texts: &'a SessionTexts,
    /// 宿主（Pebrel 的 pane）会把阶段帧画成连接卡片。那时远端不再重复打一份
    /// 人读的阶段行——卡片就在同一块屏幕上，两份文本只会互相抢位置。
    pub(super) card: bool,
}

/// 会话引导的完整远端命令行（交给 `ssh -tt` 的最后一个参数）。
pub(super) fn session_command(input: &Session<'_>) -> String {
    let texts = input.texts;
    let args = input.claude_args.iter().map(|arg| sh_quote(arg)).collect::<Vec<_>>().join(" ");
    let script = fill(
        SESSION,
        &[
            ("RUN_ID", sh_quote(input.run_id)),
            ("KEY", sh_quote(input.project_key)),
            ("PORT", input.remote_port.to_string()),
            ("HOST", sh_quote(input.host)),
            ("USER", sh_quote(input.windows_user)),
            ("HOST_KEY", sh_quote(input.host_key)),
            ("MIRROR", sh_quote(input.mirror)),
            ("PROBE", sh_quote(input.probe)),
            ("MSG_TUNNEL", sh_quote(&texts.tunnel)),
            ("MSG_PROBE", sh_quote(&texts.probe)),
            ("MSG_SESSION", sh_quote(&texts.session)),
            ("CARD", if input.card { "1" } else { "0" }.to_owned()),
            ("ERR_PROVISION", sh_quote(&texts.not_provisioned)),
            ("ERR_PROBE", sh_quote(&texts.probe_failed)),
            ("ERR_MIRROR", sh_quote(&texts.mirror_missing)),
            ("EXIT_NOT_PROVISIONED", EXIT_NOT_PROVISIONED.to_string()),
            ("EXIT_PROBE_FAILED", EXIT_PROBE_FAILED.to_string()),
            ("EXIT_MIRROR_MISSING", EXIT_MIRROR_MISSING.to_string()),
            ("PROMPT", sh_quote(input.prompt)),
            ("ARGS", args),
        ],
    );
    launcher(&script)
}

/// 把整段脚本包进各家登录 shell 都能原样转交给 `/bin/sh` 的一行命令。
pub(super) fn launcher(script: &str) -> String {
    let encoded = base64::engine::general_purpose::STANDARD.encode(script);
    format!("exec /bin/sh -c 'eval \"$(printf %s {encoded} | base64 -d)\"'")
}

/// 把本地化文本转成 printf 模板：文本里的 `%` 与 `\` 原样输出，`\0` 换成 `%s`。
///
/// 调用方用 `\0` 作为只有服务器才知道的那个值的占位（见 [`SessionTexts`]）。
pub(super) fn printf_template(text: &str) -> String {
    text.replace('\\', r"\\").replace('%', "%%").replace('\u{0}', "%s")
}

/// 一次性替换模板里的 `@@NAME@@`：替换进去的值不会再被当成占位符，所以项目路径、
/// 提示词或用户参数里出现同样的字样也不会被二次展开。
fn fill(template: &str, values: &[(&str, String)]) -> String {
    let mut output = String::with_capacity(template.len() + 4096);
    let mut rest = template;
    while let Some(start) = rest.find("@@") {
        output.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find("@@") else {
            output.push_str(&rest[start..]);
            return output;
        };
        let name = &after[..end];
        match values.iter().find(|(key, _)| *key == name) {
            Some((_, value)) => output.push_str(value),
            None => panic!("remote script placeholder @@{name}@@ has no value"),
        }
        rest = &after[end + 2..];
    }
    output.push_str(rest);
    output
}

/// 预检/下发输出里的一行 `PEBREL|key|value…`。
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Report {
    Home(String),
    Mirror(String),
    Port(u16),
    Claude(String),
    Auth(bool),
    Ready,
    Error { code: String, detail: String },
}

pub(super) fn parse_report(line: &str) -> Option<Report> {
    let rest = line.trim_end_matches(['\r', '\n']).strip_prefix("PEBREL|")?;
    let (key, value) = rest.split_once('|').unwrap_or((rest, ""));
    Some(match key {
        "home" => Report::Home(value.to_owned()),
        "mirror" => Report::Mirror(value.to_owned()),
        "port" => Report::Port(value.parse().ok()?),
        "claude" => Report::Claude(value.to_owned()),
        "auth" => Report::Auth(value == "ok"),
        "ready" => Report::Ready,
        "error" => {
            let (code, detail) = value.split_once('|').unwrap_or((value, ""));
            Report::Error { code: code.to_owned(), detail: detail.to_owned() }
        },
        _ => return None,
    })
}

const PROVISION: &str = r#"set -u
umask 077
run_id=@@RUN_ID@@
rel=@@REL@@
key=@@KEY@@
ports=@@PORTS@@
emit() { printf 'PEBREL|%s\n' "$1"; }
fail() { printf 'PEBREL|error|%s|%s\n' "$1" "${2:-}"; exit 3; }
limit() { if command -v timeout >/dev/null 2>&1; then timeout 20 "$@"; else "$@"; fi; }
quote() { printf "'%s'" "$(printf '%s' "$1" | sed "s/'/'\\\\''/g")"; }
[ -n "${HOME:-}" ] && [ -d "$HOME" ] || fail home_missing
base="$HOME/.pebrel-remote"
mkdir -p "$base/run" "$base/projects" 2>/dev/null || fail state_unwritable "$base"
chmod 700 "$base" "$base/run" "$base/projects" 2>/dev/null
# Remove connection directories whose session shell is gone; a directory without a
# pid is still being set up by another connection for its first ten minutes.
for d in "$base"/run/*; do
  [ -d "$d" ] || continue
  if [ -f "$d/pid" ]; then
    kill -0 "$(cat "$d/pid" 2>/dev/null)" 2>/dev/null && continue
  elif [ -n "$(find "$d" -prune -mmin -10 2>/dev/null)" ]; then
    continue
  fi
  rm -rf "$d"
done
command -v ssh >/dev/null 2>&1 || fail remote_ssh_missing
claude=''
claude_path=''
pick() {
  if [ -n "$1" ] && [ -x "$1" ]; then claude=$1; claude_path=${2:-$PATH}; return 0; fi
  return 1
}
if ! pick "$(command -v claude 2>/dev/null)" "$PATH"; then
  login=${SHELL:-/bin/sh}
  for mode in -lc -ic; do
    out=$(limit "$login" "$mode" 'printf "\nPEBREL_PATH=%s\nPEBREL_CLAUDE=%s\n" "$PATH" "$(command -v claude)"' </dev/null 2>/dev/null)
    found=$(printf '%s\n' "$out" | sed -n 's/^PEBREL_CLAUDE=//p' | tail -n 1)
    found_path=$(printf '%s\n' "$out" | sed -n 's/^PEBREL_PATH=//p' | tail -n 1)
    pick "$found" "$found_path" && break
  done
fi
if [ -z "$claude" ]; then
  for c in "$HOME/.local/bin/claude" "$HOME/.claude/local/claude" "$HOME/.npm-global/bin/claude" "$HOME/.bun/bin/claude" /usr/local/bin/claude /opt/homebrew/bin/claude; do
    pick "$c" "${c%/*}:$PATH" && break
  done
fi
[ -n "$claude" ] || fail claude_missing
version=$(PATH=$claude_path; export PATH; limit "$claude" --version 2>&1 </dev/null | head -n 1)
case "$version" in
  *[0-9].[0-9]*) ;;
  *) fail claude_broken "$version" ;;
esac
status=$(PATH=$claude_path; export PATH; limit "$claude" auth status 2>/dev/null </dev/null)
case "$status" in
  *'"loggedIn": true'*|*'"loggedIn":true'*) auth=ok ;;
  *'"loggedIn": false'*|*'"loggedIn":false'*) fail claude_logged_out ;;
  *) auth=unknown ;;
esac
mirror="$HOME/@@MIRROR_ROOT@@/$rel"
mkdir -p "$mirror" 2>/dev/null || fail mirror_unwritable "$mirror"
listening() {
  hex=$(printf '%04X' "$1")
  cat /proc/net/tcp /proc/net/tcp6 2>/dev/null |
    awk -v want=":$hex" '$4 == "0A" && substr($2, length($2) - 4) == want { found = 1 } END { exit !found }'
}
port=''
for p in $ports; do
  if ! listening "$p"; then port=$p; break; fi
done
[ -n "$port" ] || fail no_free_port
run="$base/run/$run_id"
mkdir "$run" 2>/dev/null || fail state_unwritable "$run"
cat > "$run/identity" <<'PEBREL_IDENTITY'
@@IDENTITY@@
PEBREL_IDENTITY
chmod 600 "$run/identity"
{
  printf 'PATH=%s\nexport PATH\n' "$(quote "$claude_path")"
  printf 'PEBREL_CLAUDE=%s\n' "$(quote "$claude")"
} > "$run/env"
emit "home|$HOME"
emit "mirror|$mirror"
emit "port|$port"
emit "claude|$version"
emit "auth|$auth"
emit "ready"
"#;

const SESSION: &str = r#"set -u
umask 077
run_id=@@RUN_ID@@
key=@@KEY@@
port=@@PORT@@
host=@@HOST@@
win_user=@@USER@@
host_key=@@HOST_KEY@@
mirror=@@MIRROR@@
probe=@@PROBE@@
msg_tunnel=@@MSG_TUNNEL@@
msg_probe=@@MSG_PROBE@@
msg_session=@@MSG_SESSION@@
card=@@CARD@@
err_provision=@@ERR_PROVISION@@
err_probe=@@ERR_PROBE@@
err_mirror=@@ERR_MIRROR@@
prompt=@@PROMPT@@
base="$HOME/.pebrel-remote"
run="$base/run/$run_id"
proj="$base/projects/$key"
wd=''
fail() {
  code=$1
  shift
  printf '\r\n'
  # shellcheck disable=SC2059 -- the first argument is a printf template built locally.
  printf "$@"
  printf '\r\n'
  frame failed
  exit "$code"
}
# 阶段帧：宿主（Pebrel）把它翻成 pane 内的连接卡片；别的终端忽略这条 OSC。
frame() { printf '\033]777;pebrel-remote-claude;%s;%s\007' "$1" "$host"; }
# 人读的阶段行只在没有卡片的终端（普通 shell、管道、别的终端）里打；
# 有卡片时这些字就写在同一块屏幕的卡片背后，只会变成残留文本。
note() { [ "$card" = 1 ] || printf '%s\r\n' "$1"; }
[ -f "$run/identity" ] || fail @@EXIT_NOT_PROVISIONED@@ '%s' "$err_provision"
cleanup() {
  trap - EXIT HUP INT TERM
  [ -n "$wd" ] && kill "$wd" 2>/dev/null
  rm -rf "$run"
  # Hand the stable per-project entry to another live connection of the same project.
  if grep -qF "$run/" "$proj/ssh_config" 2>/dev/null; then
    next=''
    for d in "$base"/run/*; do
      [ -f "$d/project" ] && [ "$(cat "$d/project")" = "$key" ] || continue
      [ -f "$d/pid" ] && kill -0 "$(cat "$d/pid")" 2>/dev/null && next=$d
    done
    if [ -n "$next" ]; then
      printf 'Include "%s/ssh_config"\n' "$next" > "$proj/ssh_config.$$" &&
        mv -f "$proj/ssh_config.$$" "$proj/ssh_config"
    else
      rm -f "$proj/ssh_config"
    fi
  fi
}
trap cleanup EXIT
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM
printf '%s\n' "$$" > "$run/pid"
printf '%s\n' "$key" > "$run/project"
printf '[127.0.0.1]:%s %s\n' "$port" "$host_key" > "$run/known_hosts"
{
  printf 'Host windows\n  HostName 127.0.0.1\n  Port %s\n  User "%s"\n' "$port" "$win_user"
  printf '  IdentityFile "%s/identity"\n  UserKnownHostsFile "%s/known_hosts"\n' "$run" "$run"
  printf '  IdentitiesOnly yes\n  BatchMode yes\n  StrictHostKeyChecking yes\n  ConnectTimeout 10\n'
  printf '  ServerAliveInterval 15\n  ServerAliveCountMax 3\n  ForwardAgent no\n  ForwardX11 no\n'
  printf '  ClearAllForwardings yes\n  ControlMaster no\n  ControlPath none\n  LogLevel ERROR\n'
} > "$run/ssh_config"
mkdir -p "$proj"
printf 'Include "%s/ssh_config"\n' "$run" > "$proj/ssh_config.$$" &&
  mv -f "$proj/ssh_config.$$" "$proj/ssh_config"
note "$msg_tunnel"
# 自检可能要重试几次（每次 1s 退避），卡片就停在这一步。
frame probe
tries=0
while :; do
  out=$(ssh -F "$proj/ssh_config" windows "$probe" </dev/null 2>&1)
  case "$out" in *PEBREL_SSH_READY*) break ;; esac
  tries=$((tries + 1))
  if [ "$tries" -ge 8 ]; then
    fail @@EXIT_PROBE_FAILED@@ "$err_probe" "$(printf '%s\n' "$out" | tr -d '\r' | tail -n 3 | tr '\n' ' ')"
  fi
  sleep 1
done
detail=$(printf '%s\n' "$out" | tr -d '\r' | sed -n 's/^.*PEBREL_SSH_READY *//p' | head -n 1)
# shellcheck disable=SC2059 -- msg_probe is a printf template built locally.
note "$(printf "$msg_probe" "$detail")"
cd "$mirror" 2>/dev/null || fail @@EXIT_MIRROR_MISSING@@ '%s' "$err_mirror"
. "$run/env"
# Watchdog: the server may not notice a powered-off or disconnected computer for hours,
# so end the session after about two minutes without a successful loopback login.
(
  trap - EXIT HUP INT TERM
  misses=0
  while sleep 30; do
    if ssh -F "$run/ssh_config" -o ConnectTimeout=15 windows 'exit 0' >/dev/null 2>&1; then
      misses=0
    else
      misses=$((misses + 1))
      if [ "$misses" -ge 4 ]; then kill -HUP 0; exit 0; fi
    fi
  done
) </dev/null >/dev/null 2>&1 &
wd=$!
frame session
note "$msg_session"
# Ctrl+C belongs to Claude Code while it runs; a handler (not SIG_IGN) is reset for the child.
trap ':' INT
frame ready
"$PEBREL_CLAUDE" --append-system-prompt "$prompt" @@ARGS@@
exit $?
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_parse_values_errors_and_noise() {
        assert_eq!(parse_report("PEBREL|home|/home/cc\n"), Some(Report::Home("/home/cc".into())));
        assert_eq!(
            parse_report("PEBREL|mirror|/home/cc/x/y\r\n"),
            Some(Report::Mirror("/home/cc/x/y".into()))
        );
        assert_eq!(parse_report("PEBREL|port|51234"), Some(Report::Port(51234)));
        assert_eq!(parse_report("PEBREL|auth|ok"), Some(Report::Auth(true)));
        assert_eq!(parse_report("PEBREL|auth|unknown"), Some(Report::Auth(false)));
        assert_eq!(parse_report("PEBREL|ready"), Some(Report::Ready));
        assert_eq!(
            parse_report("PEBREL|error|claude_missing|"),
            Some(Report::Error { code: "claude_missing".into(), detail: String::new() })
        );
        // 登录 shell 的问候语与不带值的行不是报告。
        assert_eq!(parse_report("Welcome to Ubuntu"), None);
        assert_eq!(parse_report("PEBREL|port|not-a-port"), None);
    }

    #[test]
    fn printf_templates_escape_percent_and_backslash_and_take_the_server_value() {
        assert_eq!(printf_template("ready 50% \\ done"), "ready 50%% \\\\ done");
        assert_eq!(printf_template("check passed (\u{0})."), "check passed (%s).",);
    }

    #[test]
    fn placeholders_are_filled_once() {
        // 值里的 `@@…@@` 不会被二次展开（项目路径、提示词、用户参数都会走到这里）。
        let filled = fill("a=@@A@@ b=@@B@@", &[("A", "@@B@@".into()), ("B", "x".into())]);
        assert_eq!(filled, "a=@@B@@ b=x");
    }

    #[test]
    fn session_command_embeds_arguments_and_stays_one_token() {
        let texts = SessionTexts {
            tunnel: "tunnel".into(),
            probe: "probe (%s)".into(),
            session: "session".into(),
            not_provisioned: "missing".into(),
            probe_failed: "failed (%s)".into(),
            mirror_missing: "mirror".into(),
        };
        let command = session_command(&Session {
            run_id: "0f1e2d3c4b5a",
            project_key: "aabbccdd",
            remote_port: 51234,
            host: "box.example",
            windows_user: "user",
            host_key: "ssh-ed25519 AAAA test",
            mirror: "/home/cc/pebrel-remote/c/proj",
            probe: "powershell -EncodedCommand AAAA",
            prompt: "prompt",
            claude_args: &["--resume".to_owned(), "it's fine".to_owned()],
            texts: &texts,
            card: true,
        });
        // 命令行本体是 base64：登录 shell 只看到 echo/base64/sh 三个词。
        let encoded = command
            .strip_prefix("exec /bin/sh -c 'eval \"$(printf %s ")
            .and_then(|rest| rest.strip_suffix(" | base64 -d)\"'"))
            .expect("the launcher must be a single base64-wrapped command");
        let decoded =
            String::from_utf8(base64::engine::general_purpose::STANDARD.decode(encoded).unwrap())
                .unwrap();
        // 用户参数按 POSIX sh 单引号转义后进脚本。
        assert!(decoded.contains("'--resume'"), "{decoded}");
        assert!(decoded.contains(r"'it'\''s fine'"), "{decoded}");
        assert!(decoded.contains("'prompt'"), "{decoded}");
        assert!(decoded.contains("'powershell -EncodedCommand AAAA'"), "{decoded}");
        // 阶段帧契约：远端脚本在自检/会话/接管前各发一帧，宿主据此画卡片。
        assert!(decoded.contains("host='box.example'"), "{decoded}");
        assert!(decoded.contains("pebrel-remote-claude;%s;%s"), "{decoded}");
        assert!(decoded.contains("frame probe"), "{decoded}");
        assert!(decoded.contains("frame ready"), "{decoded}");
        // 有卡片的宿主走 `note`：同一份阶段文本不再往终端打第二遍。
        assert!(decoded.contains("card=1"), "{decoded}");
        assert!(!decoded.contains(r#"printf '%s\r\n' "$msg_tunnel""#), "{decoded}");
    }

    /// 没有卡片的终端（普通 shell、管道、别的终端）照旧打印人读阶段行。
    #[test]
    fn a_host_without_a_card_still_gets_the_stage_lines() {
        let texts = SessionTexts {
            tunnel: "tunnel".into(),
            probe: "probe (%s)".into(),
            session: "session".into(),
            not_provisioned: "missing".into(),
            probe_failed: "failed (%s)".into(),
            mirror_missing: "mirror".into(),
        };
        let command = session_command(&Session {
            run_id: "0f1e2d3c4b5a",
            project_key: "aabbccdd",
            remote_port: 51234,
            host: "box.example",
            windows_user: "user",
            host_key: "ssh-ed25519 AAAA test",
            mirror: "/home/cc/pebrel-remote/c/proj",
            probe: "probe",
            prompt: "prompt",
            claude_args: &[],
            texts: &texts,
            card: false,
        });
        let encoded = command
            .strip_prefix("exec /bin/sh -c 'eval \"$(printf %s ")
            .and_then(|rest| rest.strip_suffix(" | base64 -d)\"'"))
            .expect("the launcher must be a single base64-wrapped command");
        let decoded =
            String::from_utf8(base64::engine::general_purpose::STANDARD.decode(encoded).unwrap())
                .unwrap();
        assert!(decoded.contains("card=0"), "{decoded}");
        assert!(decoded.contains(r#"note "$msg_tunnel""#), "{decoded}");
    }
}
