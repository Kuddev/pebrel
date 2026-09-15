use crate::gpui_shell::ssh_settings::SshValidationError;

#[derive(Clone, Debug, Default)]
pub(super) enum AboutUpdateState {
    #[default]
    Idle,
    Checking,
    UpToDate(String),
    Available(String),
    Failed(String),
}

#[derive(Clone, Debug)]
pub(super) enum ProviderStatus {
    Saved,
    Added,
    AtLeastOneRequired,
    Deleted,
    ApiKeySaved,
    Testing,
    TestResult { outcome: crate::provider_test::ProviderTestOutcome, elapsed_ms: u64 },
    CodexConfirmation,
    AppliedToCodex(std::path::PathBuf),
    Error(String),
}

impl ProviderStatus {
    pub(in crate::gpui_shell) fn is_error(&self) -> bool {
        match self {
            Self::AtLeastOneRequired | Self::Error(_) => true,
            Self::TestResult { outcome, .. } => !outcome.is_success(),
            _ => false,
        }
    }

    pub(in crate::gpui_shell) fn text(&self, language: crate::display::UiLanguage) -> String {
        match self {
            Self::Saved => language.tr("settings.status.provider_saved").into(),
            Self::Added => language.tr("settings.status.provider_added").into(),
            Self::AtLeastOneRequired => {
                language.tr("settings.status.provider_at_least_one").into()
            },
            Self::Deleted => language
                .tr("settings.status.provider_deleted")
                .into(),
            Self::ApiKeySaved => language
                .tr("settings.status.provider_api_key_saved")
                .into(),
            Self::Testing => language.tr("settings.status.testing_connection").into(),
            Self::TestResult { outcome, elapsed_ms } => {
                format!("{} · {elapsed_ms} ms", language.provider_test_message(outcome))
            },
            Self::CodexConfirmation => language
                .tr("settings.status.codex_confirmation")
                .into(),
            Self::AppliedToCodex(path) => format!(
                "{}: {}",
                language.tr("settings.status.applied_to_codex"),
                path.display()
            ),
            Self::Error(error) => format!(
                "{}: {error}",
                language.tr("settings.status.operation_failed")
            ),
        }
    }
}
#[derive(Clone, Debug)]
pub(super) enum BackupCompletion {
    Exported(std::path::PathBuf),
    Restored,
    Pushed(String),
    Pulled(String),
}

#[derive(Clone, Debug)]
pub(super) enum BackupStatus {
    PassphraseTooShort,
    SelectionRequired,
    Processing,
    RemoteConfigSaved,
    CredentialEmpty,
    CredentialUnsupported,
    CredentialSaved,
    Completed(BackupCompletion),
    Error(String),
}

impl BackupStatus {
    pub(in crate::gpui_shell) fn is_error(&self) -> bool {
        matches!(
            self,
            Self::PassphraseTooShort
                | Self::SelectionRequired
                | Self::CredentialEmpty
                | Self::CredentialUnsupported
                | Self::Error(_)
        )
    }

    pub(in crate::gpui_shell) fn text(&self, language: crate::display::UiLanguage) -> String {
        match self {
            Self::PassphraseTooShort => language
                .tr("settings.status.passphrase_too_short")
                .into(),
            Self::SelectionRequired => language
                .tr("settings.status.selection_required")
                .into(),
            Self::Processing => language.tr("settings.status.processing").into(),
            Self::RemoteConfigSaved => {
                language.tr("settings.status.remote_config_saved").into()
            },
            Self::CredentialEmpty => {
                language.tr("settings.status.credential_empty").into()
            },
            Self::CredentialUnsupported => language
                .tr("settings.status.credential_unsupported")
                .into(),
            Self::CredentialSaved => language
                .tr("settings.status.credential_saved")
                .into(),
            Self::Completed(BackupCompletion::Exported(path)) => format!(
                "{}: {}",
                language.tr("settings.status.backup_exported"),
                path.display()
            ),
            Self::Completed(BackupCompletion::Restored) => language
                .tr("settings.status.backup_restored")
                .into(),
            Self::Completed(BackupCompletion::Pushed(location)) => {
                format!("{} {location}", language.tr("settings.status.pushed_to"))
            },
            Self::Completed(BackupCompletion::Pulled(name)) => format!(
                "{} {name} {}",
                language.tr("settings.status.restored_from"),
                language.tr("settings.status.restart_required_notice")
            ),
            Self::Error(error) => format!(
                "{}: {error}",
                language.tr("settings.status.backup_failed")
            ),
        }
    }
}

#[derive(Debug)]
pub(super) enum TerminalImportError {
    Scan(String),
    NoSupportedTerminal,
    Load(String),
    Import(String),
    Save(String),
}

impl TerminalImportError {
    pub(super) fn text(self, language: crate::display::UiLanguage) -> String {
        match self {
            Self::Scan(error) => format!(
                "{}: {error}",
                language.tr("settings.status.scan_failed")
            ),
            Self::NoSupportedTerminal => language
                .tr("settings.status.no_supported_terminal")
                .into(),
            Self::Load(error) => format!(
                "{}: {error}",
                language.tr("settings.status.load_failed")
            ),
            Self::Import(error) => format!(
                "{}: {error}",
                language.tr("settings.status.import_failed")
            ),
            Self::Save(error) => format!(
                "{}: {error}",
                language.tr("settings.status.save_failed")
            ),
        }
    }
}

#[derive(Clone, Debug)]
pub(in crate::gpui_shell) enum SshStatus {
    LibraryProcessing,
    LibraryReloaded,
    LibraryExported,
    LibraryImported { added: usize, skipped: usize },
    LibraryImportedPartial { added: usize, error: String },
    Saved(String),
    Pinned,
    Imported(usize),
    Opening(String),
    DeleteCommitted { hidden_config: bool },
    CleanupPartial(String),
    Restored(String),
    Validation(SshValidationError),
    PersistFailed(String),
    DeleteFailed(String),
    UndoFailed(String),
    TestStartFailed(String),
    ProfileLoadFailed(String),
    ProfileSaveFailed(String),
    HostListSaveFailed(String),
    CredentialSaveFailed(String),
    SavedWithCleanupError { destination: String, error: String },
    Error(String),
}

impl SshStatus {
    pub(in crate::gpui_shell) fn is_error(&self) -> bool {
        matches!(
            self,
            Self::LibraryImportedPartial { .. }
                | Self::CleanupPartial(_)
                | Self::Validation(_)
                | Self::PersistFailed(_)
                | Self::DeleteFailed(_)
                | Self::UndoFailed(_)
                | Self::TestStartFailed(_)
                | Self::ProfileLoadFailed(_)
                | Self::ProfileSaveFailed(_)
                | Self::HostListSaveFailed(_)
                | Self::CredentialSaveFailed(_)
                | Self::SavedWithCleanupError { .. }
                | Self::Error(_)
        )
    }

    pub(in crate::gpui_shell) fn text(&self, language: crate::display::UiLanguage) -> String {
        match self {
            Self::LibraryImportedPartial { added, error } => language.format(crate::i18n::Message::HostsImportedPartial,
                &[("added", &added.to_string()), ("error", error)]),
            Self::LibraryProcessing => language.text(crate::i18n::Message::HostsProcessing).to_owned(),
            Self::LibraryReloaded => language.text(crate::i18n::Message::HostsReloaded).to_owned(),
            Self::LibraryExported => language.text(crate::i18n::Message::HostsExported).to_owned(),
            Self::LibraryImported { added, skipped } => language.format(crate::i18n::Message::HostsImported,
                &[("added", &added.to_string()), ("skipped", &skipped.to_string())]),

            Self::Saved(destination) => {
                format!("{} {destination}", language.tr("common.saved"))
            },
            Self::Pinned => language.tr("settings.status.pin_updated").into(),
            Self::Imported(count) => format!(
                "{} {count} {}",
                language.tr("settings.status.imported_config"),
                language.tr("settings.status.config_aliases")
            ),
            Self::Opening(host) => {
                format!("{} {host}…", language.tr("settings.status.opening"))
            },
            Self::DeleteCommitted { hidden_config: true } => language
                .tr("settings.status.delete_hidden_config")
                .into(),
            Self::DeleteCommitted { hidden_config: false } => language
                .tr("settings.status.delete_committed")
                .into(),
            Self::CleanupPartial(details) => format!(
                "{}: {details}",
                language.tr("settings.status.cleanup_partial")
            ),
            Self::Restored(host) => {
                format!("{} {host}", language.tr("settings.status.restored"))
            },
            Self::Validation(error) => error.text(language).into(),
            Self::PersistFailed(error) => format!(
                "{}: {error}",
                language.tr("settings.status.persist_failed")
            ),
            Self::DeleteFailed(error) => format!(
                "{}: {error}",
                language.tr("settings.status.delete_failed")
            ),
            Self::UndoFailed(error) => {
                format!("{}: {error}", language.tr("settings.status.undo_failed"))
            },
            Self::TestStartFailed(error) => format!(
                "{}: {error}",
                language.tr("settings.status.test_start_failed")
            ),
            Self::ProfileLoadFailed(error) => format!(
                "{}: {error}",
                language.tr("settings.status.profile_load_failed")
            ),
            Self::ProfileSaveFailed(error) => format!(
                "{}: {error}",
                language.tr("settings.status.profile_save_failed")
            ),
            Self::HostListSaveFailed(error) => format!(
                "{}: {error}",
                language.tr("settings.status.host_list_save_failed")
            ),
            Self::CredentialSaveFailed(error) => format!(
                "{}: {error}",
                language.tr("settings.status.credential_save_failed")
            ),
            Self::SavedWithCleanupError { destination, error } => format!(
                "{} {destination}, {}: {error}",
                language.tr("common.saved"),
                language.tr("settings.status.cleanup_error")
            ),
            Self::Error(error) => {
                format!("{}: {error}", language.tr("settings.status.ssh_operation_failed"))
            },
        }
    }
}
