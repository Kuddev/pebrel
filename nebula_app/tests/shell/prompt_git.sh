# Run after sourcing the production prompt_git.sh in either Bash or Zsh.
set -eu
unset GIT_DIR GIT_WORK_TREE GIT_COMMON_DIR GIT_CEILING_DIRECTORIES GIT_DISCOVERY_ACROSS_FILESYSTEM
export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1
fixture=$(mktemp -d)
trap 'rm -rf -- "$fixture"' EXIT
repo="$fixture/repo space 中文"
mkdir "$repo" "$fixture/outside"
command git init -q -b main "$repo"
command git -C "$repo" -c user.name=Prompt -c user.email=prompt@example.invalid commit -q --allow-empty -m fixture
log="$fixture/queries"
: > "$log"
git() {
    if [[ ${1-} == rev-parse ]]; then
        printf x >> "$log"
        if [[ ${slow_query:-0} == 1 ]]; then sleep 0.05; fi
    fi
    command git "$@"
}
query_count() {
    local queries=''
    IFS= read -r queries < "$log" || :
    [[ ${#queries} == "$1" ]] || { printf 'expected %s queries, got %s\n' "$1" "${#queries}" >&2; exit 80; }
}
expect_branch() {
    __pebrel_prompt_git
    [[ $__pebrel_prompt_branch == "$1" ]] || { printf 'expected branch %s, got %s\n' "$1" "$__pebrel_prompt_branch" >&2; exit 81; }
}
cd "$repo"
expect_branch main
query_count 1
for ((i=0; i<100; i++)); do expect_branch main; done
query_count 1
command git checkout -qb next
expect_branch next
query_count 1
command git checkout -q --detach
expect_branch HEAD
query_count 1
command git checkout -q main
expect_branch main
literal='$(printf${IFS}PEBREL_REVIEW_MARKER)%F{red}'
command git checkout -qb "$literal"
expect_branch "$literal"
command git checkout -q main
expect_branch main
query_count 1

# New nested metadata, deletion, and symlinked logical cwd must not use a
# repository discovered for the previous filesystem context.
mkdir "$repo/nested"
cd "$repo/nested"
expect_branch main
command git init -q -b nested
expect_branch nested
mv .git .git-saved
expect_branch main
mv .git-saved .git
expect_branch nested
if ln -s "$repo/nested" "$fixture/logical-link" && [[ -d $fixture/logical-link ]]; then
    cd "$fixture/logical-link"
    expect_branch nested
else
    printf 'SKIP: native symlink directories unavailable\n'
fi

command git -C "$repo" worktree add -q -b linked "$fixture/linked worktree"
cd "$fixture/linked worktree"
expect_branch linked
command git checkout -qb linked-next
expect_branch linked-next

command git clone -q --bare "$repo" "$fixture/bare.git"
cd "$fixture/bare.git"
expect_branch main

# Submodules have a .git file; let Git resolve it instead of assuming .git/HEAD.
command git -C "$repo" -c protocol.file.allow=always submodule add -q "$repo" sub
cd "$repo/sub"
expect_branch main

cd "$fixture/outside"
expect_branch ''
export GIT_DIR="$repo/.git" GIT_WORK_TREE="$repo"
expect_branch main
export GIT_DIR=''
expect_branch ''
unset GIT_DIR GIT_WORK_TREE
expect_branch ''

# A repo created in the current cwd is visible without changing directory.
command git init -q -b new-repo
expect_branch new-repo
saved_head=''
IFS= read -r saved_head < .git/HEAD
printf 'broken head\n' > .git/HEAD
expect_branch ''
printf '%s\n' "$saved_head" > .git/HEAD
expect_branch new-repo

# Updating the target of a gitfile invalidates the cached absolute directory.
mkdir "$fixture/gitfile"
printf 'gitdir: ../repo space 中文/.git\n' > "$fixture/gitfile/.git"
cd "$fixture/gitfile"
expect_branch main
printf 'gitdir: ../outside/.git\n' > .git
expect_branch new-repo

# Failed discovery is cached, and re-evaluated after an environment change.
export GIT_DIR="$fixture/missing"
expect_branch ''
before=''
IFS= read -r before < "$log" || :
for ((i=0; i<20; i++)); do expect_branch ''; done
query_count "${#before}"

# A deliberately slow Git command is only paid for once in a stable context.
slow_query=1
export GIT_DIR="$repo/.git"
expect_branch main
IFS= read -r before < "$log" || :
for ((i=0; i<100; i++)); do expect_branch main; done
query_count "${#before}"
printf 'PASS: cached discovery, live HEAD, detached/unborn branches, worktree, submodule, bare repo, environment, metadata changes, errors and slow Git\n'
