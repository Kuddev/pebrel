//! 路径来源复用文件匹配器；shell 转义只由命令上下文负责。

use nebula_completions::command_context::{CommandContext, ShellSyntax};
use nebula_completions::semantic::{Context, Source};
use nebula_completions::{CompletionOptions, SemanticSuggestion, Span, SuggestionKind};

use crate::directory_history::DirectoryHistory;
use crate::display::suggest_engine::Input;

pub(crate) fn complete(
    input: &Input<'_>,
    syntax: ShellSyntax,
    history: &DirectoryHistory,
    style: crate::display::CompletionStyle,
    semantic: Option<&Context>,
    cancelled: &dyn Fn() -> bool,
) -> (Vec<SemanticSuggestion>, Option<String>) {
    if cancelled() {
        return (Vec::new(), None);
    }
    let parsed;
    let context = if let Some(semantic) = semantic {
        semantic.input()
    } else {
        parsed = CommandContext::parse(input.line, input.line.len(), syntax);
        let Some(context) = parsed.as_ref() else { return (Vec::new(), None) };
        context
    };
    let want_dir = semantic.map_or_else(
        || crate::display::nebula_path_wants_directory(input.line),
        |context| matches!(context.source, Source::Paths { directories_only: true }),
    );
    let prefix = context.prefix();
    let mut cwd = input.cwd.to_owned();
    if let Some(semantic) = semantic {
        for directory in &semantic.directories {
            if directory.is_empty() {
                continue;
            }
            if input.env.is_this_machine() {
                let path = std::path::Path::new(directory);
                if !path.is_absolute() && cwd.is_empty() {
                    return (Vec::new(), None);
                }
                cwd = std::path::Path::new(&cwd).join(path).to_string_lossy().into_owned();
            } else if directory.starts_with('/') {
                cwd = directory.clone();
            } else if cwd.starts_with('/') {
                cwd = format!("{}/{directory}", cwd.trim_end_matches('/'));
            } else {
                return (Vec::new(), None);
            }
        }
    }
    let make = |path: &str, is_dir| {
        context.candidate(path).map(|suggestion| {
            SemanticSuggestion::with_kind(
                suggestion,
                if is_dir { SuggestionKind::Directory } else { SuggestionKind::File },
            )
        })
    };
    if !input.env.is_this_machine() {
        // 方言未知时只插入通用字面量；绝不能落到宿主目录扫描。
        if !input.env.can_query_remote_paths()
            || crate::display::nebula_is_command_position(input.line) && !prefix.contains('/')
        {
            return (Vec::new(), None);
        }
        let Some(request) = crate::remote_dirs::path_request(prefix, &cwd) else {
            return (Vec::new(), None);
        };
        let Some(entries) = crate::remote_dirs::lookup(input.env, &request.dir) else {
            return (Vec::new(), Some(request.dir));
        };
        let candidates = crate::remote_dirs::candidates(&request, &entries)
            .into_iter()
            .take_while(|_| !cancelled())
            .filter(|(_, is_dir)| !want_dir || *is_dir)
            .filter_map(|(suffix, is_dir)| make(&format!("{prefix}{suffix}"), is_dir))
            .take(256)
            .collect();
        return if cancelled() { (Vec::new(), None) } else { (candidates, None) };
    }
    let expanded;
    let prefix = if context.expands_home() {
        expanded = nebula_completions::file::expand_home(prefix);
        let Some(expanded) = expanded.as_deref() else { return (Vec::new(), None) };
        expanded
    } else {
        prefix
    };
    if !std::path::Path::new(prefix).has_root() && cwd.is_empty() {
        return (Vec::new(), None);
    }
    let mut candidates = Vec::new();
    // 旧访问记录仍可优先补到深层目录，但必须经过同一转义规则。
    if semantic.is_none()
        && want_dir
        && prefix
            .chars()
            .all(|ch| ch.is_alphanumeric() || matches!(ch, '/' | '\\' | ':' | '.' | '_' | '-'))
        && let Some(suffix) = history.hint_with_cancel(&format!("cd {prefix}"), &cwd, cancelled)
        && let Some(candidate) = make(
            &crate::platform::local_paths::completion_spelling(
                &format!("{prefix}{suffix}"),
                syntax,
            ),
            true,
        )
    {
        candidates.push(candidate);
        if style != crate::display::CompletionStyle::Popup {
            return if cancelled() { (Vec::new(), None) } else { (candidates, None) };
        }
    }
    let options = CompletionOptions {
        case_sensitive: crate::platform::local_paths::completion_case_sensitive(),
        ..Default::default()
    };
    let matches = nebula_completions::file::complete_literal_with_cancel(
        want_dir,
        Span::new(0, prefix.len()),
        prefix,
        &[cwd.as_str()],
        &options,
        cancelled,
    );
    if cancelled() {
        return (Vec::new(), None);
    }
    let matches = if want_dir { history.rank_file_suggestions(matches, &cwd) } else { matches };
    for item in matches.into_iter().take_while(|_| !cancelled()) {
        let path = crate::platform::local_paths::completion_spelling(&item.path, syntax);
        if let Some(candidate) = make(&path, item.is_dir)
            && !candidates.iter().any(|seen| seen.suggestion.value == candidate.suggestion.value)
        {
            candidates.push(candidate);
        }
        if candidates.len() == 256 {
            break;
        }
    }
    if cancelled() { (Vec::new(), None) } else { (candidates, None) }
}
