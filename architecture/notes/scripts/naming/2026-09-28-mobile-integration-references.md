# Mobile dependency and session references in name checks

## Status
Implemented with positive and negative regression coverage.

## Context
The naming policy rejects unrelated product comparisons while preserving legal
attribution and functional compatibility identifiers. Android introduces native
dependency metadata, Gradle module references and remote session commands that
were absent from the original Rust-focused compatibility rules.

## Evidence
The staged-name check rejected the actual Gradle dependency directive, the native
C++ include, pinned source URLs and remote session commands. Removing those
identifiers would break the build or change the program executed on an SSH host.
The owning sources are `mobile/android/app/build.gradle.kts`, the terminal JNI
adapter and `mobile/android/app/src/main/.../connection/SshSessionMode.kt`.

## Decision
Keep the existing policy and add recognition for the necessary reference forms
at their owning paths. Remove only each matched reference before scanning the
remaining line. Preserve third-party notices and remove incidental references
from ordinary descriptions, diagnostics and fixture labels.

## Rejected alternatives
- Renaming dependency headers or remote executable commands to pass a text check.
- Exempting whole source directories, files or all quoted strings.
- Removing license and source attribution from the Android distribution.

## Consequences
The same line can contain both a legitimate reference and a prohibited comparison;
the latter still fails. New integrations need evidence before extending these
forms. Source URLs remain bounded to dependency metadata, not ordinary prose.

## Validation
The existing naming test module covers real build/session references, appended
comparisons, unrelated metadata fields and the same text outside its owning path.
The staged and pending-commit checks remain required.

## Supersedes
None.

## Revisit when
New dependency metadata or remote command interfaces require additional forms.
