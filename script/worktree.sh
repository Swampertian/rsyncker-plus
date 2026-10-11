#!/usr/bin/env sh
set -eu

usage() {
    printf '%s\n' \
        'Uso:' \
        '  script/worktree.sh create <agente> <tarefa>' \
        '  script/worktree.sh integrate <agente> <tarefa>' \
        '  script/worktree.sh remove <agente> <tarefa>' \
        '  script/worktree.sh list'
}

valid_name() {
    case "$1" in
        ''|*[!a-z0-9-]*|[!a-z0-9]*) return 1 ;;
        *) return 0 ;;
    esac
}

if ! root=$(git rev-parse --show-toplevel 2>/dev/null); then
    printf '%s\n' 'Erro: execute este comando dentro de um checkout Git.' >&2
    exit 2
fi
cd "$root"

command=${1:-}
case "$command" in
    list)
        if [ "$#" -ne 1 ]; then
            usage >&2
            exit 2
        fi
        git worktree list
        ;;
    create|integrate|remove)
        if [ "$#" -ne 3 ] || ! valid_name "$2" || ! valid_name "$3"; then
            usage >&2
            printf '%s\n' 'Agente e tarefa devem usar apenas a-z, 0-9 e hífen.' >&2
            exit 2
        fi
        agent=$2
        task=$3
        branch="agent/$agent/$task"
        path=".worktrees/$agent-$task"
        if [ "$command" = create ]; then
            mkdir -p .worktrees
            git worktree add -b "$branch" "$path" HEAD
        elif [ "$command" = integrate ]; then
            integration_branch=${RSYNC_TUI_INTEGRATION_BRANCH:-master}
            if [ "$(git branch --show-current)" != "$integration_branch" ]; then
                printf 'Erro: execute a integração no checkout da branch %s.\n' "$integration_branch" >&2
                exit 1
            fi
            integration_status=$(git status --porcelain)
            if [ "$(git -C "$path" branch --show-current)" != "$branch" ]; then
                printf 'Erro: worktree ausente ou não está na branch %s.\n' "$branch" >&2
                exit 1
            fi
            if [ -n "$(git -C "$path" status --porcelain)" ]; then
                printf '%s\n' 'Erro: a worktree do agente precisa estar limpa e com alterações commitadas.' >&2
                exit 1
            fi
            if ! git show-ref --verify --quiet "refs/heads/$integration_branch" \
                || ! git show-ref --verify --quiet "refs/heads/$branch"; then
                printf '%s\n' 'Erro: branch de integração ou branch do agente não encontrada.' >&2
                exit 1
            fi
            if ! git merge-base --is-ancestor "$integration_branch" "$branch"; then
                printf '%s\n' 'Erro: branches divergiram; resolva a base antes de integrar.' >&2
                exit 1
            fi
            commits=$(git rev-list --count "$integration_branch..$branch")
            if [ "$commits" -eq 0 ]; then
                printf '%s\n' 'Erro: branch do agente não contém commits novos.' >&2
                exit 1
            fi
            (
                cd "$path"
                CARGO_TARGET_DIR="target/agent-$agent" script/check.sh
            )
            if [ "$(git status --porcelain)" != "$integration_status" ]; then
                printf '%s\n' 'Erro: o checkout de integração mudou durante as verificações.' >&2
                exit 1
            fi
            if ! git merge --no-ff --no-edit "$branch"; then
                git merge --abort || true
                printf '%s\n' 'Erro: merge não aplicado; conflitos precisam de resolução.' >&2
                exit 1
            fi
            git worktree remove "$path"
            git branch -d "$branch"
        else
            git worktree remove "$path"
        fi
        ;;
    *)
        usage >&2
        exit 2
        ;;
esac
