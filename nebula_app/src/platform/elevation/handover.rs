//! 管理员启动请求与可取消的 UI 投递；原生传输由平台后端拥有。

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub(crate) use windows::start_or_forward;

use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use crate::config::ui_config::Program;
use serde::{Deserialize, Serialize};
use tokio::sync::oneshot;

type Reply = Result<(), String>;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Request {
    pub cwd: Option<PathBuf>,
    pub command: Option<Program>,
    pub shell_id: Option<String>,
}

impl Request {
    fn validate(&self) -> io::Result<()> {
        let valid_text = |value: &str| !value.contains('\0');
        if self.cwd.as_ref().is_some_and(|path| {
            !path.is_absolute() || path.to_str().is_none_or(|value| !valid_text(value))
        }) || self.shell_id.as_deref().is_some_and(|id| id.trim().is_empty() || !valid_text(id))
            || self.command.as_ref().is_some_and(|command| {
                command.program().trim().is_empty()
                    || !valid_text(command.program())
                    || command.args().iter().any(|argument| !valid_text(argument))
            })
        {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid elevated launch"));
        }
        Ok(())
    }
}

/// 排队期间取消会撤回启动；一旦 UI 开始执行，就不再回退创建第二份进程。
pub(crate) struct Dispatch {
    request: Request,
    pending: AtomicBool,
    reply: Mutex<Option<oneshot::Sender<Reply>>>,
}

impl Dispatch {
    pub(crate) fn new(request: Request) -> (Arc<Self>, oneshot::Receiver<Reply>) {
        let (sender, receiver) = oneshot::channel();
        (
            Arc::new(Self {
                request,
                pending: AtomicBool::new(true),
                reply: Mutex::new(Some(sender)),
            }),
            receiver,
        )
    }

    pub(crate) fn run(&self, operation: impl FnOnce(&Request) -> Reply) {
        if self.pending.swap(false, Ordering::AcqRel) {
            let result = operation(&self.request);
            if let Some(reply) = self.reply.lock().unwrap_or_else(|e| e.into_inner()).take() {
                let _ = reply.send(result);
            }
        }
    }

    fn cancel(&self) {
        self.pending.store(false, Ordering::Release);
    }
}

pub(crate) enum Startup {
    Resident(Server),
    Forwarded,
}

/// 一个管理员进程只拥有一个休眠中的管道任务；App 退出时唤醒并释放原生句柄。
pub(crate) struct Server(Option<oneshot::Sender<()>>);

impl Drop for Server {
    fn drop(&mut self) {
        if let Some(stop) = self.0.take() {
            let _ = stop.send(());
        }
    }
}

#[cfg(not(windows))]
pub(crate) fn start_or_forward(
    _request: Request,
    _config_file: Option<&Path>,
    _send: impl Fn(Arc<Dispatch>) -> bool + Send + 'static,
) -> io::Result<Startup> {
    Err(io::Error::new(io::ErrorKind::Unsupported, "elevated launch handover requires Windows"))
}
