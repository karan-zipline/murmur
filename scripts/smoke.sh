#!/usr/bin/env bash
set -euo pipefail

ROOT="$(mktemp -d)"
cleanup() {
  if [[ -n "${DAEMON_PID:-}" ]]; then
    kill "${DAEMON_PID}" >/dev/null 2>&1 || true
  fi
  rm -rf "${ROOT}" >/dev/null 2>&1 || true
}
trap cleanup EXIT

BIN_DIR="${ROOT}/bin"
mkdir -p "${BIN_DIR}"

cat > "${BIN_DIR}/claude" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail

while IFS= read -r line; do
  if [[ -z "${line// }" ]]; then
    continue
  fi
  echo '{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"(smoke fake claude) ok"}]}}'
done
EOF
chmod +x "${BIN_DIR}/claude"

DEMO_ROOT="${ROOT}/demo"
mkdir -p "${DEMO_ROOT}"

ORIGIN="${DEMO_ROOT}/origin.git"
git init --bare "${ORIGIN}" >/dev/null

SEED="${DEMO_ROOT}/seed"
git clone "${ORIGIN}" "${SEED}" >/dev/null
(cd "${SEED}" && git checkout -b main >/dev/null)
echo "hello" > "${SEED}/README.md"
(cd "${SEED}" && git add . >/dev/null)
(cd "${SEED}" && git -c user.name=Smoke -c user.email=smoke@example.com commit -m "init" >/dev/null)
(cd "${SEED}" && git push -u origin main >/dev/null)

FUGUE_DIR="${ROOT}/fugue"
mkdir -p "${FUGUE_DIR}"

PATH="${BIN_DIR}:${PATH}" FUGUE_DIR="${FUGUE_DIR}" cargo run -p fugue -- server start --foreground >/dev/null 2>&1 &
DAEMON_PID="$!"

LOG_PATH="${FUGUE_DIR}/fugue.log"
SOCK_PATH="${FUGUE_DIR}/fugue.sock"

deadline=$((SECONDS + 10))
while [[ "${SECONDS}" -lt "${deadline}" ]]; do
  if [[ -S "${SOCK_PATH}" ]] && [[ -f "${LOG_PATH}" ]] && grep -q "daemon ready" "${LOG_PATH}"; then
    break
  fi
  sleep 0.05
done
if [[ ! -S "${SOCK_PATH}" ]]; then
  echo "daemon did not start (socket missing)" >&2
  exit 1
fi

PATH="${BIN_DIR}:${PATH}" FUGUE_DIR="${FUGUE_DIR}" cargo run -p fugue -- project add demo --remote-url "${ORIGIN}" --max-agents 1 >/dev/null

ISSUE_ID="$(PATH="${BIN_DIR}:${PATH}" FUGUE_DIR="${FUGUE_DIR}" cargo run -p fugue -- issue create demo "Smoke issue" | tr -d '\n')"
if [[ -z "${ISSUE_ID}" ]]; then
  echo "issue create failed (empty id)" >&2
  exit 1
fi

PATH="${BIN_DIR}:${PATH}" FUGUE_DIR="${FUGUE_DIR}" cargo run -p fugue -- orchestration start demo >/dev/null

deadline=$((SECONDS + 15))
AGENT_ID=""
while [[ "${SECONDS}" -lt "${deadline}" ]]; do
  out="$(PATH="${BIN_DIR}:${PATH}" FUGUE_DIR="${FUGUE_DIR}" cargo run -p fugue -- agent list)"
  AGENT_ID="$(echo "${out}" | awk 'NF{print $1; exit}')"
  if [[ -n "${AGENT_ID}" ]]; then
    break
  fi
  sleep 0.1
done
if [[ -z "${AGENT_ID}" ]]; then
  echo "no agent spawned" >&2
  exit 1
fi

PATH="${BIN_DIR}:${PATH}" FUGUE_DIR="${FUGUE_DIR}" cargo run -p fugue -- agent done "${AGENT_ID}" >/dev/null

deadline=$((SECONDS + 15))
while [[ "${SECONDS}" -lt "${deadline}" ]]; do
  out="$(PATH="${BIN_DIR}:${PATH}" FUGUE_DIR="${FUGUE_DIR}" cargo run -p fugue -- issue get demo "${ISSUE_ID}" || true)"
  if echo "${out}" | grep -q "^status[[:space:]]\+closed$"; then
    break
  fi
  sleep 0.1
done

out="$(PATH="${BIN_DIR}:${PATH}" FUGUE_DIR="${FUGUE_DIR}" cargo run -p fugue -- issue get demo "${ISSUE_ID}")"
if ! echo "${out}" | grep -q "^status[[:space:]]\+closed$"; then
  echo "issue did not close; issue get output:" >&2
  echo "${out}" >&2
  exit 1
fi

PATH="${BIN_DIR}:${PATH}" FUGUE_DIR="${FUGUE_DIR}" cargo run -p fugue -- server shutdown >/dev/null

echo "ok"

