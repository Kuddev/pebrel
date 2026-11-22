//! Matching semantics and bounded ranking shared by cached and streamed queries.

use super::*;

pub(super) enum QueryMatcher {
    Plain { terms: Vec<String>, whole_word: bool, match_case: bool },
    Regex(regex::Regex),
}

impl QueryMatcher {
    pub(super) fn compile(query: &str, options: FileSearchOptions) -> Result<Self, String> {
        if query.len() > 4096 {
            return Err("The file search query is too long (maximum 4096 bytes)".to_owned());
        }
        if options.regex {
            let source = if options.whole_word {
                format!(r"(?:^|[^\p{{L}}\p{{N}}_])(?:{query})(?:$|[^\p{{L}}\p{{N}}_])")
            } else {
                query.to_owned()
            };
            return regex::RegexBuilder::new(&source)
                .case_insensitive(!options.match_case)
                .size_limit(256 * 1024)
                .dfa_size_limit(256 * 1024)
                .build()
                .map(Self::Regex)
                .map_err(|error| error.to_string());
        }
        let terms = query
            .split_whitespace()
            .map(|term| if options.match_case { term.to_owned() } else { term.to_lowercase() })
            .collect();
        Ok(Self::Plain { terms, whole_word: options.whole_word, match_case: options.match_case })
    }

    pub(super) fn matches(&self, entry: &IndexedPath) -> bool {
        match self {
            Self::Regex(regex) => regex.is_match(&entry.key),
            Self::Plain { terms, whole_word, match_case } => {
                let haystack = if *match_case { &entry.key } else { &entry.key_folded };
                terms.iter().all(|term| {
                    if *whole_word {
                        contains_whole_word(haystack, term)
                    } else {
                        haystack.contains(term)
                    }
                })
            },
        }
    }
}

fn contains_whole_word(haystack: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return true;
    }
    haystack.match_indices(needle).any(|(start, matched)| {
        let before = haystack[..start].chars().next_back();
        let after = haystack[start + matched.len()..].chars().next();
        !before.is_some_and(is_word_char) && !after.is_some_and(is_word_char)
    })
}

fn is_word_char(character: char) -> bool {
    character.is_alphanumeric() || character == '_'
}

fn result_score(entry: &IndexedPath, query: &str, match_case: bool) -> u8 {
    let name = if match_case { &entry.name } else { &entry.name_folded };
    if name == query {
        0
    } else if name.starts_with(query) {
        1
    } else if name.contains(query) {
        2
    } else {
        3
    }
}

pub(super) struct BestMatches {
    rows: Vec<(u8, IndexedPath)>,
    bytes: usize,
    total: usize,
    query: String,
    match_case: bool,
    pub limited: bool,
}

fn compare(left: &(u8, IndexedPath), right: &(u8, IndexedPath)) -> std::cmp::Ordering {
    left.0
        .cmp(&right.0)
        .then(right.1.is_dir.cmp(&left.1.is_dir))
        .then(left.1.name_folded.cmp(&right.1.name_folded))
        .then(left.1.key_folded.cmp(&right.1.key_folded))
}

impl BestMatches {
    pub fn new(request: &SearchRequest) -> Self {
        let rows = Vec::with_capacity(MAX_SEARCH_RESULTS);
        Self {
            bytes: rows.capacity() * std::mem::size_of::<(u8, IndexedPath)>(),
            rows,
            total: 0,
            query: if request.options.match_case {
                request.query.clone()
            } else {
                request.query.to_lowercase()
            },
            match_case: request.options.match_case,
            limited: false,
        }
    }

    pub fn add(&mut self, entry: IndexedPath) {
        self.total += 1;
        let bytes = entry.heap_bytes();
        let ranked = (result_score(&entry, &self.query, self.match_case), entry);
        let position =
            self.rows.binary_search_by(|row| compare(row, &ranked)).unwrap_or_else(|pos| pos);
        if position >= MAX_SEARCH_RESULTS {
            self.limited = true;
            return;
        }
        while self.rows.len() >= MAX_SEARCH_RESULTS || self.bytes + bytes > memory::MATCH_LIMIT {
            self.limited = true;
            if self.rows.len() <= position {
                return;
            }
            let Some((_, removed)) = self.rows.pop() else { return };
            self.bytes -= removed.heap_bytes();
        }
        self.bytes += bytes;
        self.rows.insert(position, ranked);
    }

    pub fn result(&self, request: &SearchRequest) -> (FileSearchResult, bool) {
        let mut memory = FileSearchMemory::results();
        let array_bytes = self.rows.len() * std::mem::size_of::<FileRow>();
        let mut rows = Vec::new();
        let mut limited = false;
        if memory.grow(array_bytes) {
            rows.reserve_exact(self.rows.len());
            for (_, entry) in &self.rows {
                let bytes = entry.path.as_os_str().len()
                    + entry.name.len()
                    + entry.guest_path.as_ref().map_or(0, String::len);
                if memory.bytes() + bytes > memory::RESULT_LIMIT || !memory.grow(bytes) {
                    limited = true;
                    break;
                }
                rows.push(FileRow {
                    path: entry.path.clone(),
                    guest_path: entry.guest_path.clone(),
                    name: entry.name.clone(),
                    depth: 0,
                    is_dir: entry.is_dir,
                    expanded: false,
                    is_parent: false,
                    ignored: false,
                });
            }
        } else {
            limited = true;
        }
        (
            FileSearchResult {
                epoch: request.epoch,
                generation: request.generation,
                query: request.query.clone(),
                options: request.options,
                rows,
                total: self.total,
                error: None,
                memory,
                complete: true,
            },
            limited,
        )
    }
}

#[cfg(test)]
pub(super) fn search_entries(entries: &[IndexedPath], request: &SearchRequest) -> FileSearchResult {
    let mut best = BestMatches::new(request);
    match QueryMatcher::compile(&request.query, request.options) {
        Ok(matcher) => {
            for entry in entries {
                if matcher.matches(entry) {
                    best.add(entry.clone());
                }
            }
            best.result(request).0
        },
        Err(error) => {
            let mut result = best.result(request).0;
            result.error = Some(error);
            result
        },
    }
}
