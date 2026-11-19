//! Built-in development recipes. They are templates inserted for review, never auto-executed.

use super::SavedCommand;
use crate::i18n::{Message, UiLanguage};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CommandPlatform {
    Windows,
    Mac,
    Posix,
}

pub(crate) fn commands(language: UiLanguage, platform: CommandPlatform) -> Vec<SavedCommand> {
    use CommandPlatform::*;
    let mut commands = Vec::new();
    if platform == Windows {
        commands.push(SavedCommand {
            id: "builtin:docker_install_windows".into(),
            name: language.text(Message::CommandsBuiltinDockerInstallWindows).into(),
            command: "winget install --exact --id Docker.DockerDesktop".into(),
            append_enter: false,
        });
    }
    if platform == Mac {
        commands.push(SavedCommand {
            id: "builtin:docker_install_mac".into(),
            name: language.text(Message::CommandsBuiltinDockerInstallMac).into(),
            command: "brew install --cask docker".into(),
            append_enter: false,
        });
    }
    if platform == Posix {
        commands.push(SavedCommand {
            id: "builtin:docker_install_ubuntu".into(),
            name: language.text(Message::CommandsBuiltinDockerInstallUbuntu).into(),
            command: "sudo apt-get update && sudo apt-get install docker.io docker-compose-v2"
                .into(),
            append_enter: false,
        });
    }
    if platform == Posix {
        commands.push(SavedCommand {
            id: "builtin:docker_install_fedora".into(),
            name: language.text(Message::CommandsBuiltinDockerInstallFedora).into(),
            command: "sudo dnf install moby-engine docker-compose".into(),
            append_enter: false,
        });
    }
    commands.push(SavedCommand {
        id: "builtin:docker_logs".into(),
        name: language.text(Message::CommandsBuiltinDockerLogs).into(),
        command: "docker logs --follow --tail 100 container_name".into(),
        append_enter: false,
    });
    commands.push(SavedCommand {
        id: "builtin:docker_exec".into(),
        name: language.text(Message::CommandsBuiltinDockerExec).into(),
        command: "docker exec -it container_name sh".into(),
        append_enter: false,
    });
    commands.push(SavedCommand {
        id: "builtin:compose_up".into(),
        name: language.text(Message::CommandsBuiltinComposeUp).into(),
        command: "docker compose up -d --build".into(),
        append_enter: false,
    });
    if platform == Windows {
        commands.push(SavedCommand {
            id: "builtin:conda_install_windows".into(),
            name: language.text(Message::CommandsBuiltinCondaInstallWindows).into(),
            command: "winget install --exact --id Anaconda.Miniconda3".into(),
            append_enter: false,
        });
    }
    if platform == Mac {
        commands.push(SavedCommand {
            id: "builtin:conda_install_mac".into(),
            name: language.text(Message::CommandsBuiltinCondaInstallMac).into(),
            command: "brew install --cask miniconda".into(),
            append_enter: false,
        });
    }
    if platform == Posix {
        commands.push(SavedCommand {
            id: "builtin:conda_install_linux".into(),
            name: language.text(Message::CommandsBuiltinCondaInstallLinux).into(),
            command: "curl -fL -o miniconda-installer.sh \"https://repo.anaconda.com/miniconda/Miniconda3-latest-Linux-$(uname -m).sh\" && bash miniconda-installer.sh".into(),
            append_enter: false,
        });
    }
    commands.push(SavedCommand {
        id: "builtin:conda_create".into(),
        name: language.text(Message::CommandsBuiltinCondaCreate).into(),
        command: "conda create -n dev python=3.12".into(),
        append_enter: false,
    });
    commands.push(SavedCommand {
        id: "builtin:conda_export".into(),
        name: language.text(Message::CommandsBuiltinCondaExport).into(),
        command: "conda env export --from-history > environment.yml".into(),
        append_enter: false,
    });
    commands.push(SavedCommand {
        id: "builtin:conda_restore".into(),
        name: language.text(Message::CommandsBuiltinCondaRestore).into(),
        command: "conda env create -f environment.yml".into(),
        append_enter: false,
    });
    if platform == Windows {
        commands.push(SavedCommand {
            id: "builtin:python_install_windows".into(),
            name: language.text(Message::CommandsBuiltinPythonInstallWindows).into(),
            command: "winget install --exact --id Python.Python.3.14".into(),
            append_enter: false,
        });
    }
    if platform == Mac {
        commands.push(SavedCommand {
            id: "builtin:python_install_mac".into(),
            name: language.text(Message::CommandsBuiltinPythonInstallMac).into(),
            command: "brew install python".into(),
            append_enter: false,
        });
    }
    if platform == Posix {
        commands.push(SavedCommand {
            id: "builtin:python_install_ubuntu".into(),
            name: language.text(Message::CommandsBuiltinPythonInstallUbuntu).into(),
            command: "sudo apt-get update && sudo apt-get install python3 python3-pip python3-venv"
                .into(),
            append_enter: false,
        });
    }
    if platform == Posix {
        commands.push(SavedCommand {
            id: "builtin:python_install_fedora".into(),
            name: language.text(Message::CommandsBuiltinPythonInstallFedora).into(),
            command: "sudo dnf install python3 python3-pip".into(),
            append_enter: false,
        });
    }
    if platform == Windows {
        commands.push(SavedCommand {
            id: "builtin:git_install_windows".into(),
            name: language.text(Message::CommandsBuiltinGitInstallWindows).into(),
            command: "winget install --exact --id Git.Git".into(),
            append_enter: false,
        });
    }
    if platform == Mac {
        commands.push(SavedCommand {
            id: "builtin:git_install_mac".into(),
            name: language.text(Message::CommandsBuiltinGitInstallMac).into(),
            command: "brew install git".into(),
            append_enter: false,
        });
    }
    if platform == Posix {
        commands.push(SavedCommand {
            id: "builtin:git_install_ubuntu".into(),
            name: language.text(Message::CommandsBuiltinGitInstallUbuntu).into(),
            command: "sudo apt-get update && sudo apt-get install git".into(),
            append_enter: false,
        });
    }
    if platform == Posix {
        commands.push(SavedCommand {
            id: "builtin:git_install_fedora".into(),
            name: language.text(Message::CommandsBuiltinGitInstallFedora).into(),
            command: "sudo dnf install git".into(),
            append_enter: false,
        });
    }
    commands
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recipes_are_unique_reviewable_and_platform_specific() {
        for platform in [CommandPlatform::Windows, CommandPlatform::Mac, CommandPlatform::Posix] {
            let rows = commands(UiLanguage::EnUs, platform);
            assert!(rows.len() <= 13, "keep the builtin catalog small");
            let ids: std::collections::HashSet<_> = rows.iter().map(|row| &row.id).collect();
            assert_eq!(ids.len(), rows.len());
            assert!(rows.iter().all(|row| !row.append_enter && !row.command.contains('\n')));
            assert_eq!(
                rows.iter().any(|row| row.command.starts_with("winget ")),
                platform == CommandPlatform::Windows
            );
            assert!(rows.iter().any(|row| row.command == "conda create -n dev python=3.12"));
            assert!(rows.iter().any(|row| row.command == "docker compose up -d --build"));
        }
    }
}
