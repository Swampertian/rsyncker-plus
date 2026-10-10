#!/usr/bin/env sh
set -eu

usage() {
    printf '%s\n' \
        'Uso:' \
        '  script/worktree.sh create <agente> <tarefa>' \
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
    create|remove)
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
        else
            git worktree remove "$path"
        fi
        ;;
    *)
        usage >&2
        exit 2
        ;;
esac
