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

MURMUR_DIR="${ROOT}/murmur"
mkdir -p "${MURMUR_DIR}"

PATH="${BIN_DIR}:${PATH}" MURMUR_DIR="${MURMUR_DIR}" cargo run -p murmur --bin mm -- server start --foreground >/dev/null 2>&1 &
DAEMON_PID="$!"

LOG_PATH="${MURMUR_DIR}/murmur.log"
SOCK_PATH="${MURMUR_DIR}/murmur.sock"

deadline=$((SECONDS + 60))
while [[ "${SECONDS}" -lt "${deadline}" ]]; do
  if [[ -S "${SOCK_PATH}" ]] && [[ -f "${LOG_PATH}" ]]; then
    break
  fi
  sleep 0.05
done
if [[ ! -S "${SOCK_PATH}" ]]; then
  echo "daemon did not start (socket missing)" >&2
  exit 1
fi

PATH="${BIN_DIR}:${PATH}" MURMUR_DIR="${MURMUR_DIR}" cargo run -p murmur --bin mm -- project add demo --remote-url "${ORIGIN}" --max-agents 1 >/dev/null

ISSUE_ID="$(PATH="${BIN_DIR}:${PATH}" MURMUR_DIR="${MURMUR_DIR}" cargo run -p murmur --bin mm -- issue create --project demo "Smoke issue" | tr -d '\n')"
if [[ -z "${ISSUE_ID}" ]]; then
  echo "issue create failed (empty id)" >&2
  exit 1
fi

PATH="${BIN_DIR}:${PATH}" MURMUR_DIR="${MURMUR_DIR}" cargo run -p murmur --bin mm -- project start demo >/dev/null

deadline=$((SECONDS + 15))
AGENT_ID=""
while [[ "${SECONDS}" -lt "${deadline}" ]]; do
  out="$(PATH="${BIN_DIR}:${PATH}" MURMUR_DIR="${MURMUR_DIR}" cargo run -p murmur --bin mm -- agent list)"
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

PATH="${BIN_DIR}:${PATH}" MURMUR_DIR="${MURMUR_DIR}" MURMUR_AGENT_ID="${AGENT_ID}" cargo run -p murmur --bin mm -- agent done >/dev/null

deadline=$((SECONDS + 15))
while [[ "${SECONDS}" -lt "${deadline}" ]]; do
  out="$(PATH="${BIN_DIR}:${PATH}" MURMUR_DIR="${MURMUR_DIR}" cargo run -p murmur --bin mm -- issue show --project demo "${ISSUE_ID}" || true)"
  if echo "${out}" | grep -q "^status[[:space:]]\\+closed$"; then
    break
  fi
  sleep 0.1
done

out="$(PATH="${BIN_DIR}:${PATH}" MURMUR_DIR="${MURMUR_DIR}" cargo run -p murmur --bin mm -- issue show --project demo "${ISSUE_ID}")"
if ! echo "${out}" | grep -q "^status[[:space:]]\\+closed$"; then
  echo "issue did not close; issue show output:" >&2
  echo "${out}" >&2
  exit 1
fi

PATH="${BIN_DIR}:${PATH}" MURMUR_DIR="${MURMUR_DIR}" cargo run -p murmur --bin mm -- server stop >/dev/null

echo "ok"
