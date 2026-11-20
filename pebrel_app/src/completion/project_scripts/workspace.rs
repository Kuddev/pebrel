//! Bounded declarative workspace discovery and script intersection.

use std::collections::BTreeSet;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use pebrel_completions::semantic::{Context, PackageManager, Source};

struct Package {
    name: String,
    path: String,
    scripts: BTreeSet<String>,
    dependencies: BTreeSet<String>,
}

struct Discovery<'a> {
    cancelled: &'a dyn Fn() -> bool,
    deadline: Instant,
    bytes: usize,
    directories: usize,
}

impl Discovery<'_> {
    fn ready(&self) -> bool {
        !(self.cancelled)() && Instant::now() < self.deadline
    }

    fn read(&mut self, path: &Path, limit: usize) -> Option<Vec<u8>> {
        if !self.ready() {
            return None;
        }
        let metadata = std::fs::metadata(path).ok()?;
        if !metadata.is_file()
            || metadata.len() as usize > limit
            || self.bytes + metadata.len() as usize > 4 * 1024 * 1024
        {
            return None;
        }
        let mut bytes = Vec::new();
        std::fs::File::open(path).ok()?.take(limit as u64 + 1).read_to_end(&mut bytes).ok()?;
        self.bytes += bytes.len();
        (bytes.len() <= limit && self.bytes <= 4 * 1024 * 1024 && self.ready()).then_some(bytes)
    }

    fn manifest(&mut self, path: &Path) -> Option<serde_json::Value> {
        serde_json::from_slice(&self.read(path, 1024 * 1024)?).ok()
    }
}

fn package(json: &serde_json::Value, path: String) -> Package {
    let scripts = json
        .get("scripts")
        .and_then(serde_json::Value::as_object)
        .into_iter()
        .flatten()
        .filter(|(name, value)| !name.starts_with('-') && value.is_string())
        .map(|(name, _)| name.clone())
        .collect();
    let dependencies = ["dependencies", "devDependencies", "optionalDependencies"]
        .into_iter()
        .flat_map(|key| json.get(key).and_then(serde_json::Value::as_object).into_iter().flatten())
        .map(|(name, _)| name.clone())
        .collect();
    Package {
        name: json.get("name").and_then(serde_json::Value::as_str).unwrap_or(&path).to_owned(),
        path,
        scripts,
        dependencies,
    }
}

pub(super) fn read(
    cwd: &Path,
    context: &Context,
    cancelled: &dyn Fn() -> bool,
) -> Option<Vec<String>> {
    let mut discovery = Discovery {
        cancelled,
        deadline: Instant::now() + Duration::from_millis(500),
        bytes: 0,
        directories: 0,
    };
    let (root, json, yaml) = find_root(cwd, context, &mut discovery)?;
    let patterns = patterns(&json, yaml.as_deref(), context.project.manager)?;
    let mut packages = Vec::new();
    collect(&root, &root, &patterns, &mut packages, &mut discovery)?;
    if context.project.include_root
        || context.project.manager == PackageManager::Pnpm && yaml.is_none()
    {
        packages.push(package(&json, ".".into()));
    }
    packages.sort_by(|a, b| a.name.cmp(&b.name).then(a.path.cmp(&b.path)));
    packages.dedup_by(|a, b| a.path == b.path);
    finish(&packages, context)
}

fn find_root(
    cwd: &Path,
    context: &Context,
    discovery: &mut Discovery<'_>,
) -> Option<(PathBuf, serde_json::Value, Option<String>)> {
    let mut root = context.directories.last().map_or_else(|| cwd.to_owned(), |dir| cwd.join(dir));
    let mut nearest = None;
    for _ in 0..64 {
        if !discovery.ready() || !root.is_dir() {
            return None;
        }
        let manifest = root.join("package.json");
        if manifest.exists() {
            let json = discovery.manifest(&manifest)?;
            let pnpm = root.join("pnpm-workspace.yaml");
            let yaml = if pnpm.exists() {
                Some(String::from_utf8(discovery.read(&pnpm, 64 * 1024)?).ok()?)
            } else {
                None
            };
            if context.project.manager == PackageManager::Pnpm {
                if yaml.is_some() {
                    return Some((root, json, yaml));
                }
                if nearest.is_none() {
                    nearest = Some((root.clone(), json, None));
                }
            } else if !patterns(&json, yaml.as_deref(), context.project.manager)?.is_empty() {
                return Some((root, json, yaml));
            }
        }
        if root.join("node_modules").is_dir() || !root.pop() {
            return nearest;
        }
    }
    None
}

pub(in crate::completion) fn finish_catalog(
    catalog: &serde_json::Value,
    context: &Context,
) -> Option<Vec<String>> {
    let packages: Vec<_> = catalog
        .as_array()?
        .iter()
        .take(513)
        .map(|entry| Some(package(entry.get("manifest")?, entry.get("path")?.as_str()?.to_owned())))
        .collect::<Option<_>>()?;
    if packages.len() > 512 {
        return None;
    }
    finish(&packages, context)
}

fn finish(packages: &[Package], context: &Context) -> Option<Vec<String>> {
    if matches!(context.source, Source::Workspaces) {
        let path_names = context.value_prefix().starts_with('.')
            || context.value_prefix().contains('/') && !context.value_prefix().starts_with('@');
        return Some(
            packages
                .iter()
                .map(|p| {
                    if path_names && context.project.manager != PackageManager::Yarn {
                        if context.value_prefix().starts_with("./") {
                            format!("./{}", p.path)
                        } else {
                            p.path.clone()
                        }
                    } else {
                        p.name.clone()
                    }
                })
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect(),
        );
    }
    let chosen = select(packages, context)?;
    if chosen.is_empty() {
        return Some(Vec::new());
    }
    let mut scripts: Option<BTreeSet<String>> = None;
    for index in chosen {
        let next = &packages[index].scripts;
        scripts = Some(match scripts {
            None => next.clone(),
            Some(previous)
                if context.project.allow_missing
                    || context.project.manager == PackageManager::Pnpm =>
            {
                previous.union(next).cloned().collect()
            },
            Some(previous) => previous.intersection(next).cloned().collect(),
        });
    }
    Some(scripts.unwrap_or_default().into_iter().collect())
}

pub(in crate::completion) fn patterns(
    json: &serde_json::Value,
    yaml: Option<&str>,
    manager: PackageManager,
) -> Option<Vec<String>> {
    if manager == PackageManager::Pnpm {
        let Some(yaml) = yaml else {
            return Some(vec!["**".into()]);
        };
        let yaml: serde_yaml::Value = serde_yaml::from_str(yaml).ok()?;
        let Some(packages) = yaml.get("packages") else {
            return Some(vec!["**".into()]);
        };
        return Some(
            packages
                .as_sequence()?
                .iter()
                .filter_map(serde_yaml::Value::as_str)
                .map(str::to_owned)
                .collect(),
        );
    }
    Some(
        json.get("workspaces")
            .map(|value| value.get("packages").unwrap_or(value))
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(serde_json::Value::as_str)
            .map(str::to_owned)
            .collect(),
    )
}

fn collect(
    root: &Path,
    directory: &Path,
    patterns: &[String],
    packages: &mut Vec<Package>,
    discovery: &mut Discovery<'_>,
) -> Option<()> {
    if !discovery.ready() || discovery.directories >= 4096 || packages.len() >= 512 {
        return None;
    }
    discovery.directories += 1;
    let mut children = Vec::new();
    for entry in std::fs::read_dir(directory).ok()? {
        if !discovery.ready() || children.len() >= 4096 {
            return None;
        }
        let entry = entry.ok()?;
        if !entry.file_type().ok()?.is_dir()
            || matches!(
                entry.file_name().to_str(),
                Some("node_modules" | ".git" | "target" | "dist" | "build")
            )
        {
            continue;
        }
        children.push(entry.path());
    }
    children.sort();
    for child in children {
        let relative = child.strip_prefix(root).ok()?.to_string_lossy().replace('\\', "/");
        let positives: Vec<_> = patterns.iter().filter(|p| !p.starts_with('!')).collect();
        if !positives.iter().any(|pattern| {
            let prefix = pattern
                .split(['*', '?', '[', '{'])
                .next()
                .unwrap_or_default()
                .trim_end_matches('/');
            prefix.is_empty()
                || prefix == relative
                || prefix.starts_with(&format!("{relative}/"))
                || relative.starts_with(&format!("{prefix}/"))
        }) {
            continue;
        }
        let matches = positives.iter().any(|pattern| {
            glob::Pattern::new(pattern).is_ok_and(|p| {
                p.matches_with(
                    &relative,
                    glob::MatchOptions {
                        require_literal_separator: true,
                        require_literal_leading_dot: true,
                        case_sensitive: true,
                    },
                )
            })
        });
        let excluded = patterns.iter().filter_map(|p| p.strip_prefix('!')).any(|pattern| {
            glob::Pattern::new(pattern).is_ok_and(|p| {
                p.matches_with(
                    &relative,
                    glob::MatchOptions {
                        require_literal_separator: true,
                        require_literal_leading_dot: true,
                        case_sensitive: true,
                    },
                )
            })
        });
        if matches && !excluded && child.join("package.json").exists() {
            let json = discovery.manifest(&child.join("package.json"))?;
            packages.push(package(&json, relative));
        }
        collect(root, &child, patterns, packages, discovery)?;
    }
    Some(())
}

fn select(packages: &[Package], context: &Context) -> Option<BTreeSet<usize>> {
    let selectors = &context.project.selectors;
    let only_negative = !selectors.is_empty() && selectors.iter().all(|s| s.starts_with('!'));
    let mut included = if context.project.all || only_negative {
        (0..packages.len()).collect()
    } else {
        BTreeSet::new()
    };
    let mut excluded = BTreeSet::new();
    if context.project.include_root && context.project.manager == PackageManager::Npm {
        included.extend(
            packages
                .iter()
                .enumerate()
                .filter_map(|(i, package)| (package.path == ".").then_some(i)),
        );
    }
    for selector in selectors {
        if context.project.manager == PackageManager::Yarn {
            included.extend(
                packages.iter().enumerate().filter_map(|(i, p)| (p.name == *selector).then_some(i)),
            );
            continue;
        }
        let negative = selector.starts_with('!');
        let selector = selector.strip_prefix('!').unwrap_or(selector);
        let dependents = selector.starts_with("...");
        let dependencies = selector.ends_with("...");
        let selector = selector.strip_prefix("...").unwrap_or(selector);
        let selector = selector.strip_suffix("...").unwrap_or(selector);
        let omit_self = selector.starts_with('^') || selector.ends_with('^');
        let selector = selector.trim_matches('^');
        let selector =
            selector.strip_prefix('{').and_then(|s| s.strip_suffix('}')).unwrap_or(selector);
        // Changed-since selectors require a separate Git-owned query; never treat them as all packages.
        if selector.is_empty() || selector.contains(['[', ']']) {
            return None;
        }
        let pattern = glob::Pattern::new(selector.strip_prefix("./").unwrap_or(selector)).ok()?;
        let mut seed: BTreeSet<_> = packages
            .iter()
            .enumerate()
            .filter_map(|(i, p)| {
                (pattern.matches(&p.name)
                    || pattern.matches(&p.path)
                    || context.project.manager == PackageManager::Npm
                        && p.path.starts_with(&format!("{selector}/")))
                .then_some(i)
            })
            .collect();
        if seed.is_empty()
            && context.project.manager == PackageManager::Pnpm
            && !selector.contains('/')
        {
            let unscoped: Vec<_> = packages
                .iter()
                .enumerate()
                .filter_map(|(i, p)| {
                    pattern.matches(p.name.rsplit('/').next().unwrap_or_default()).then_some(i)
                })
                .collect();
            // pnpm refuses an ambiguous unscoped literal; explicit globs may select many.
            if unscoped.len() == 1 || selector.contains(['*', '?']) {
                seed.extend(unscoped);
            }
        }
        let mut chosen = seed.clone();
        if dependencies || dependents {
            loop {
                let before = chosen.len();
                for (index, p) in packages.iter().enumerate() {
                    if dependents
                        && p.dependencies
                            .iter()
                            .any(|name| chosen.iter().any(|i| packages[*i].name == *name))
                    {
                        chosen.insert(index);
                    }
                    if dependencies && chosen.contains(&index) {
                        for (dependency, target) in packages.iter().enumerate() {
                            if p.dependencies.contains(&target.name) {
                                chosen.insert(dependency);
                            }
                        }
                    }
                }
                if chosen.len() == before {
                    break;
                }
            }
        }
        if omit_self {
            chosen = chosen.difference(&seed).copied().collect();
        }
        if negative {
            excluded.extend(chosen);
        } else {
            included.extend(chosen);
        }
    }
    Some(included.difference(&excluded).copied().collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pebrel_completions::command_context::ShellSyntax;

    #[test]
    fn pnpm_uses_its_workspace_root_and_unscoped_names_without_borrowing_npm_patterns() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("package.json"), r#"{"name":"fixture-root","workspaces":["wrong/*"],"scripts":{"root-only":"touch forbidden"}}"#).unwrap();
        let app = root.path().join("packages/app");
        std::fs::create_dir_all(&app).unwrap();
        std::fs::write(
            app.join("package.json"),
            r#"{"name":"@fixture/app","scripts":{"app-only":"touch forbidden"}}"#,
        )
        .unwrap();
        let names = |cwd: &Path, line: &str| {
            let context = Context::parse(line, line.len(), ShellSyntax::Posix).unwrap();
            read(cwd, &context, &|| false).unwrap_or_default()
        };
        assert_eq!(names(root.path(), "pnpm --filter app run app"), ["app-only"]);
        assert_eq!(names(root.path(), "pnpm --filter fixture-root run ro"), ["root-only"]);
        std::fs::write(root.path().join("pnpm-workspace.yaml"), "packages:\n  - packages/*\n")
            .unwrap();
        assert_eq!(names(&app, "pnpm --filter app run app"), ["app-only"]);
        assert!(names(root.path(), "pnpm --filter fixture-root run ro").is_empty());
        let other = root.path().join("packages/other");
        std::fs::create_dir(&other).unwrap();
        std::fs::write(
            other.join("package.json"),
            r#"{"name":"@other/app","scripts":{"other-only":"touch forbidden"}}"#,
        )
        .unwrap();
        assert!(names(root.path(), "pnpm --filter app run app").is_empty());
        assert!(!root.path().join("forbidden").exists());
    }

    #[test]
    fn workspace_selection_uses_declared_packages_and_never_executes_scripts() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("package.json"), r#"{"name":"root","workspaces":["packages/*","!packages/excluded"],"scripts":{"root-only":"touch forbidden"}}"#).unwrap();
        std::fs::write(
            root.path().join("pnpm-workspace.yaml"),
            "packages:\n  - packages/*\n  - '!packages/excluded'\n",
        )
        .unwrap();
        for (name, scripts, deps) in [
            (
                "app",
                r#"{"build:app":"touch forbidden","shared":"touch forbidden"}"#,
                r#"{"lib":"workspace:*"}"#,
            ),
            ("lib", r#"{"build:lib":"touch forbidden","shared":"touch forbidden"}"#, "{}"),
            ("excluded", r#"{"build:excluded":"touch forbidden"}"#, "{}"),
        ] {
            let path = root.path().join("packages").join(name);
            std::fs::create_dir_all(&path).unwrap();
            std::fs::write(
                path.join("package.json"),
                format!(r#"{{"name":"{name}","scripts":{scripts},"dependencies":{deps}}}"#),
            )
            .unwrap();
        }
        let names = |line: &str| {
            let context = Context::parse(line, line.len(), ShellSyntax::Posix).unwrap();
            read(root.path(), &context, &|| false).unwrap_or_default()
        };
        assert_eq!(names("npm --workspace a"), ["app", "lib"]);
        assert_eq!(names("npm -w app run bu"), ["build:app", "shared"]);
        assert_eq!(names("npm run --workspace app bu"), ["build:app", "shared"]);
        assert_eq!(names("npm --workspaces run sh"), ["shared"]);
        assert_eq!(
            names("npm --workspaces --if-present run bu"),
            ["build:app", "build:lib", "shared"]
        );
        assert_eq!(names("pnpm --filter app... run sh"), ["build:app", "build:lib", "shared"]);
        assert!(names("npm -w app --include-workspace-root run bu").is_empty());
        assert_eq!(
            names("npm -w app --include-workspace-root --if-present run bu"),
            ["build:app", "root-only", "shared"]
        );
        assert_eq!(names("pnpm --filter 'app^...' run bu"), ["build:lib", "shared"]);
        assert_eq!(names("yarn workspace app run bu"), ["build:app", "shared"]);
        assert_eq!(names("yarn workspace app bu"), ["build:app", "shared"]);
        assert!(names("npm --workspace missing run bu").is_empty());
        assert!(!root.path().join("forbidden").exists());
        let context = Context::parse("npm --workspace a", 17, ShellSyntax::Posix).unwrap();
        assert!(read(root.path(), &context, &|| true).is_none());
    }
}
