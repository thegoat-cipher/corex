#!/usr/bin/env bash
# Publish this repository to GitHub.
#
# Run:   bash scripts/publish-github.sh
# Or:    VISIBILITY=public REPO_NAME=corex bash scripts/publish-github.sh
#
# Requires a token in $TOKEN_ENV (default ~/.config/corex-github.env) containing
# GITHUB_TOKEN, as written by:
#   room-cli secret request GITHUB_TOKEN --write-env ~/.config/corex-github.env
#
# The token must be able to write repository contents:
#   - classic token: `repo` scope
#   - fine-grained token: Contents = Read and write on this repository
#
# The token is read into a shell variable only. It is never placed in argv, in
# git config, or in the pushed content. The token file is deleted after a
# successful push. The script is idempotent: an existing repository is reused.

set -euo pipefail

REPO_NAME="${REPO_NAME:-corex}"
VISIBILITY="${VISIBILITY:-}"
TOKEN_ENV="${TOKEN_ENV:-$HOME/.config/corex-github.env}"
DESCRIPTION="${DESCRIPTION:-CoreX: low-level systems language and compiler (Rust, LLVM 20, freestanding Linux x86-64)}"

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

if [[ ! -f "$TOKEN_ENV" ]]; then
    echo "error: $TOKEN_ENV not found." >&2
    echo "       room-cli secret request GITHUB_TOKEN --write-env $TOKEN_ENV" >&2
    exit 2
fi

set -a
# shellcheck disable=SC1090
. "$TOKEN_ENV"
set +a

if [[ -z "${GITHUB_TOKEN:-}" ]]; then
    echo "error: GITHUB_TOKEN is empty in $TOKEN_ENV" >&2
    exit 2
fi

# GitHub API helper: the token is supplied through curl's config on stdin, so it
# never appears in the process argument list.
api() {
    printf 'header = "Authorization: Bearer %s"\n' "$GITHUB_TOKEN" |
        curl -sS -K - \
            -H "Accept: application/vnd.github+json" \
            -H "X-GitHub-Api-Version: 2022-11-28" \
            "$@"
}

LOGIN="$(api https://api.github.com/user | python3 -c 'import json,sys; d=json.load(sys.stdin); print(d.get("login") or "ERROR: " + str(d.get("message")))')"
case "$LOGIN" in
    ERROR*|"")
        echo "error: GitHub rejected the token: $LOGIN" >&2
        exit 3
        ;;
esac
echo "account: $LOGIN"

if api "https://api.github.com/repos/$LOGIN/$REPO_NAME" | grep -q '"full_name"'; then
    echo "repository exists: $LOGIN/$REPO_NAME"
else
    if [[ -z "$VISIBILITY" ]]; then
        echo "error: repository $LOGIN/$REPO_NAME does not exist." >&2
        echo "       Create it, or set VISIBILITY=private|public to create it with this token." >&2
        exit 3
    fi
    PRIVATE=false
    [[ "$VISIBILITY" == "private" ]] && PRIVATE=true
    echo "creating $VISIBILITY repository: $LOGIN/$REPO_NAME"
    payload="$(printf '{"name":"%s","private":%s,"description":"%s","auto_init":false}' \
        "$REPO_NAME" "$PRIVATE" "$DESCRIPTION")"
    api -X POST -d "$payload" https://api.github.com/user/repos >/dev/null
fi

URL="https://github.com/$LOGIN/$REPO_NAME.git"
if git remote get-url origin >/dev/null 2>&1; then
    git remote set-url origin "$URL"
else
    git remote add origin "$URL"
fi

BRANCH="$(git branch --show-current)"
if [[ -z "$BRANCH" ]]; then
    echo "error: detached HEAD; check out a branch first" >&2
    exit 2
fi

# Credential helper is passed per command, so no secret is written to .git/config.
if ! GIT_TERMINAL_PROMPT=0 git \
    -c credential.helper='!f() { echo username=x-access-token; echo "password=$GITHUB_TOKEN"; }; f' \
    push -u origin "$BRANCH"; then
    cat >&2 <<'MSG'

error: GitHub denied the push.
       The token can read this repository but not write it.
       - classic token: add the `repo` scope
       - fine-grained token: Repository permissions -> Contents -> Read and write
       Then re-run this script.
MSG
    exit 4
fi

rm -f "$TOKEN_ENV"
echo
echo "pushed branch '$BRANCH' to https://github.com/$LOGIN/$REPO_NAME"
echo "token file removed: $TOKEN_ENV"
