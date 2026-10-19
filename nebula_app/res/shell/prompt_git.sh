# Shared Bash/Zsh prompt metadata. Git remains the authority for repository
# discovery; ordinary refreshes use shell builtin metadata checks and reads.
__pebrel_prompt_git() {
    local scan marker='' marker_value='' head=''
    __pebrel_prompt_branch=
    if [[ ${__pebrel_git_pwd-} != "$PWD" ]]; then
        __pebrel_git_physical_pwd=$(builtin pwd -P) || __pebrel_git_physical_pwd=$PWD
    fi
    if [[ -z ${GIT_DIR:-} ]]; then
        scan=${__pebrel_git_physical_pwd:-$PWD}
        while [[ -n $scan ]]; do
            if [[ -d $scan/.git ]]; then
                marker=$scan/.git
                break
            elif [[ -f $scan/.git ]]; then
                marker=$scan/.git
                marker_value=$(< "$marker") 2>/dev/null || marker_value=
                break
            fi
            if [[ -f $scan/HEAD && -d $scan/objects ]]; then
                marker=$scan
                break
            fi
            [[ $scan == / ]] && break
            scan=${scan%/*}
            [[ -n $scan ]] || scan=/
        done
    fi
    # Keep fields separate: paths and environment values may contain separators.
    # A new/deleted nested repository or a changed worktree gitfile invalidates
    # discovery even without cd. Negative discovery never starts Git repeatedly.
    if [[ ${__pebrel_git_pwd-} != "$PWD" ||
          ${__pebrel_git_marker-} != "$marker" ||
          ${__pebrel_git_marker_value-} != "$marker_value" ||
          ${__pebrel_git_env_dir-} != "${GIT_DIR+x}:${GIT_DIR-}" ||
          ${__pebrel_git_env_worktree-} != "${GIT_WORK_TREE+x}:${GIT_WORK_TREE-}" ||
          ${__pebrel_git_env_common-} != "${GIT_COMMON_DIR+x}:${GIT_COMMON_DIR-}" ||
          ${__pebrel_git_env_ceiling-} != "${GIT_CEILING_DIRECTORIES+x}:${GIT_CEILING_DIRECTORIES-}" ||
          ${__pebrel_git_env_crossfs-} != "${GIT_DISCOVERY_ACROSS_FILESYSTEM+x}:${GIT_DISCOVERY_ACROSS_FILESYSTEM-}" ||
          ${__pebrel_git_env_path-} != "$PATH" ]]; then
        __pebrel_git_pwd=$PWD
        __pebrel_git_marker=$marker
        __pebrel_git_marker_value=$marker_value
        __pebrel_git_env_dir=${GIT_DIR+x}:${GIT_DIR-}
        __pebrel_git_env_worktree=${GIT_WORK_TREE+x}:${GIT_WORK_TREE-}
        __pebrel_git_env_common=${GIT_COMMON_DIR+x}:${GIT_COMMON_DIR-}
        __pebrel_git_env_ceiling=${GIT_CEILING_DIRECTORIES+x}:${GIT_CEILING_DIRECTORIES-}
        __pebrel_git_env_crossfs=${GIT_DISCOVERY_ACROSS_FILESYSTEM+x}:${GIT_DISCOVERY_ACROSS_FILESYSTEM-}
        __pebrel_git_env_path=$PATH
        __pebrel_git_dir=
        if [[ -n $marker || -n ${GIT_DIR:-} ]]; then
            __pebrel_git_dir=$(git rev-parse --absolute-git-dir 2>/dev/null) || __pebrel_git_dir=
        fi
    fi
    if [[ -n ${__pebrel_git_dir:-} && -f $__pebrel_git_dir/HEAD ]]; then
        # Bash/Zsh read this file-only substitution without invoking cat.
        head=$(< "$__pebrel_git_dir/HEAD") 2>/dev/null || head=
        case $head in
            'ref: refs/heads/'*) __pebrel_prompt_branch=${head#ref: refs/heads/} ;;
            'ref: refs/'*) __pebrel_prompt_branch=${head#ref: refs/} ;;
            *[!0-9a-fA-F]*|'') ;;
            *)
                if [[ ${#head} == 40 || ${#head} == 64 ]]; then
                    __pebrel_prompt_branch=HEAD
                fi ;;
        esac
    fi
    return 0
}
