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
    Notice(crate::i18n::Message),
    Saved,
    Added,
    AtLeastOneRequired,
    Deleted,
    ApiKeySaved,
    Testing,
    ModelsFetched(usize),
    TestResult { outcome: crate::provider_test::ProviderTestOutcome, elapsed_ms: u64 },
    CodexConfirmation,
    AppliedToCodex(std::path::PathBuf),
    AppliedToClaude(std::path::PathBuf),
    ClaudeConfirmation,
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
            Self::Notice(message) => language.text(*message).into(),
            Self::ClaudeConfirmation => {
                language.text(crate::i18n::Message::ProvidersClaudeConfirmation).into()
            },
            Self::AppliedToClaude(path) => format!(
                "{}: {}",
                language.text(crate::i18n::Message::ProvidersAppliedClaude),
                path.display()
            ),
            Self::ModelsFetched(count) => language
                .text(crate::i18n::Message::ProvidersModelsFetched)
                .replace("{count}", &count.to_string()),
            Self::Saved => {
                language.text(crate::i18n::Message::SettingsStatusProviderSettingsSaved).into()
            },
            Self::Added => {
                language.text(crate::i18n::Message::SettingsStatusCustomProviderAdded).into()
            },
            Self::AtLeastOneRequired => {
                language.text(crate::i18n::Message::SettingsStatusKeepAtLeastOneProvider).into()
            },
            Self::Deleted => language
                .text(crate::i18n::Message::SettingsStatusProviderAndItsCredentialsDeleted)
                .into(),
            Self::ApiKeySaved => {
                language.text(crate::i18n::Message::SettingsStatusApiKeySavedToThe).into()
            },
            Self::Testing => {
                language.text(crate::i18n::Message::SettingsStatusTestingConnection).into()
            },
            Self::TestResult { outcome, elapsed_ms } => {
                format!("{} · {elapsed_ms} ms", language.provider_test_message(outcome))
            },
            Self::CodexConfirmation => {
                language.text(crate::i18n::Message::SettingsStatusClickAgainToConfirmThe).into()
            },
            Self::AppliedToCodex(path) => format!(
                "{}: {}",
                language.text(crate::i18n::Message::SettingsStatusAppliedToCodex),
                path.display()
            ),
            Self::Error(error) => format!(
                "{}: {error}",
                language.text(crate::i18n::Message::SettingsStatusOperationFailed)
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
    Notice(crate::i18n::Message),
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
            Self::Notice(message) => language.text(*message).into(),
            Self::PassphraseTooShort => {
                language.text(crate::i18n::Message::SettingsStatusTheBackupPasswordMustBe).into()
            },
            Self::SelectionRequired => {
                language.text(crate::i18n::Message::SettingsStatusSelectAtLeastOneBackup).into()
            },
            Self::Processing => {
                language.text(crate::i18n::Message::SettingsStatusProcessing).into()
            },
            Self::RemoteConfigSaved => {
                language.text(crate::i18n::Message::SettingsStatusRemoteConfigurationSaved).into()
            },
            Self::CredentialEmpty => {
                language.text(crate::i18n::Message::SettingsStatusCredentialsCannotBeEmpty).into()
            },
            Self::CredentialUnsupported => {
                language.text(crate::i18n::Message::SettingsStatusTheCurrentProtocolDoesNot).into()
            },
            Self::CredentialSaved => {
                language.text(crate::i18n::Message::SettingsStatusCredentialSavedToTheSystem).into()
            },
            Self::Completed(BackupCompletion::Exported(path)) => format!(
                "{}: {}",
                language.text(crate::i18n::Message::SettingsStatusEncryptedBackupExported),
                path.display()
            ),
            Self::Completed(BackupCompletion::Restored) => language
                .text(crate::i18n::Message::SettingsStatusBackupRestoredSomeSettingsIncluding)
                .into(),
            Self::Completed(BackupCompletion::Pushed(location)) => language.format(
                crate::i18n::Message::SettingsStatusPushedTo,
                &[("location", &location.to_string())],
            ),
            Self::Completed(BackupCompletion::Pulled(name)) => language.format(
                crate::i18n::Message::SettingsStatusRestoredFrom,
                &[("name", &name.to_string())],
            ),
            Self::Error(error) => format!(
                "{}: {error}",
                language.text(crate::i18n::Message::SettingsStatusBackupOperationFailed)
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
                language.text(crate::i18n::Message::SettingsStatusCouldNotScanTheTerminal)
            ),
            Self::NoSupportedTerminal => language
                .text(crate::i18n::Message::SettingsStatusNoSupportedTerminalProgramWas)
                .into(),
            Self::Load(error) => format!(
                "{}: {error}",
                language.text(crate::i18n::Message::SettingsStatusCouldNotReadTerminalProfiles)
            ),
            Self::Import(error) => format!(
                "{}: {error}",
                language.text(crate::i18n::Message::SettingsStatusCouldNotImportTheTerminal)
            ),
            Self::Save(error) => format!(
                "{}: {error}",
                language.text(crate::i18n::Message::SettingsStatusCouldNotSaveTerminalProfiles)
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
    Copied(String),
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
            Self::LibraryImportedPartial { added, error } => language.format(
                crate::i18n::Message::HostsImportedPartial,
                &[("added", &added.to_string()), ("error", error)],
            ),
            Self::LibraryProcessing => {
                language.text(crate::i18n::Message::HostsProcessing).to_owned()
            },
            Self::LibraryReloaded => language.text(crate::i18n::Message::HostsReloaded).to_owned(),
            Self::LibraryExported => language.text(crate::i18n::Message::HostsExported).to_owned(),
            Self::LibraryImported { added, skipped } => language.format(
                crate::i18n::Message::HostsImported,
                &[("added", &added.to_string()), ("skipped", &skipped.to_string())],
            ),

            Self::Saved(destination) => language.format(
                crate::i18n::Message::SettingsStatusSavedTo,
                &[("destination", &destination.to_string())],
            ),
            Self::Copied(label) => {
                language.format(crate::i18n::Message::HostsCopied, &[("name", label)])
            },
            Self::Pinned => {
                language.text(crate::i18n::Message::SettingsStatusPinStatusUpdated).into()
            },
            Self::Imported(count) => language.format(
                crate::i18n::Message::SettingsStatusImportedAliases,
                &[("count", &count.to_string())],
            ),
            Self::Opening(host) => language.format(
                crate::i18n::Message::SettingsStatusOpeningHost,
                &[("host", &host.to_string())],
            ),
            Self::DeleteCommitted { hidden_config: true } => language
                .text(crate::i18n::Message::SettingsStatusConfigAliasHiddenPebrelProfile)
                .into(),
            Self::DeleteCommitted { hidden_config: false } => language
                .text(crate::i18n::Message::SettingsStatusHostProfileAndCredentialsDeleted)
                .into(),
            Self::CleanupPartial(details) => format!(
                "{}: {details}",
                language.text(crate::i18n::Message::SettingsStatusHostRemovedFromTheList)
            ),
            Self::Restored(host) => language.format(
                crate::i18n::Message::SettingsStatusRestoredHost,
                &[("host", &host.to_string())],
            ),
            Self::Validation(error) => error.text(language).into(),
            Self::PersistFailed(error) => format!(
                "{}: {error}",
                language.text(crate::i18n::Message::SettingsStatusFailedToWriteSettings)
            ),
            Self::DeleteFailed(error) => format!(
                "{}: {error}",
                language.text(crate::i18n::Message::SettingsStatusFailedToDeleteHost)
            ),
            Self::UndoFailed(error) => {
                format!(
                    "{}: {error}",
                    language.text(crate::i18n::Message::SettingsStatusUndoFailed)
                )
            },
            Self::TestStartFailed(error) => format!(
                "{}: {error}",
                language.text(crate::i18n::Message::SettingsStatusCouldNotStartTheConnection)
            ),
            Self::ProfileLoadFailed(error) => format!(
                "{}: {error}",
                language.text(crate::i18n::Message::SettingsStatusFailedToLoadTheSsh)
            ),
            Self::ProfileSaveFailed(error) => format!(
                "{}: {error}",
                language.text(crate::i18n::Message::SettingsStatusFailedToSaveTheSsh)
            ),
            Self::HostListSaveFailed(error) => format!(
                "{}: {error}",
                language.text(crate::i18n::Message::SettingsStatusFailedToSaveTheHost)
            ),
            Self::CredentialSaveFailed(error) => format!(
                "{}: {error}",
                language.text(crate::i18n::Message::SettingsStatusTheProfileWasSavedBut)
            ),
            Self::SavedWithCleanupError { destination, error } => language.format(
                crate::i18n::Message::SettingsStatusSavedCleanupFailed,
                &[("destination", &destination.to_string()), ("error", &error.to_string())],
            ),
            Self::Error(error) => {
                format!(
                    "{}: {error}",
                    language.text(crate::i18n::Message::SettingsStatusSshOperationFailed)
                )
            },
        }
    }
}
