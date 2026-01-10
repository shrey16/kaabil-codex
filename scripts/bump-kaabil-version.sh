#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Usage: bump-kaabil-version.sh [--upstream VERSION] [--kaabil N] [--auto] [--from-npm|--no-npm]

Updates codex-rs/Cargo.toml workspace version to <upstream>-kaabil.<n>.

Options:
  --upstream VERSION  Upstream release version (e.g. 0.79.0)
  --kaabil N          Kaabil suffix number (default: auto)
  --auto              Fetch latest version from npm and avoid downgrades
  --from-npm          Fetch upstream version from npm (default)
  --no-npm            Do not query npm; require --upstream
USAGE
}

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cargo_toml="${repo_root}/codex-rs/Cargo.toml"

upstream=""
kaabil=""
use_npm=1
auto=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --upstream)
      upstream="${2:-}"
      shift 2
      ;;
    --kaabil)
      kaabil="${2:-}"
      shift 2
      ;;
    --auto)
      auto=1
      use_npm=1
      shift
      ;;
    --from-npm)
      use_npm=1
      shift
      ;;
    --no-npm)
      use_npm=0
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown argument: $1" >&2
      usage
      exit 2
      ;;
  esac
done

if [[ ${auto} -eq 1 && -n "${upstream}" ]]; then
  echo "--auto cannot be combined with --upstream" >&2
  exit 2
fi

if [[ ${auto} -eq 1 && ${use_npm} -eq 0 ]]; then
  echo "--auto requires npm access" >&2
  exit 2
fi

if [[ -z "${upstream}" ]]; then
  if [[ ${use_npm} -eq 1 ]]; then
    if ! command -v npm >/dev/null 2>&1; then
      echo "npm not found; pass --upstream VERSION instead" >&2
      exit 1
    fi
    upstream="$(npm view @openai/codex version)"
  else
    echo "Missing upstream version. Use --upstream VERSION." >&2
    exit 1
  fi
fi

if [[ ! "${upstream}" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "Invalid upstream version: ${upstream}" >&2
  exit 1
fi

current_version="$(python3 - "${cargo_toml}" <<'PY'
import pathlib
import re
import sys
path = pathlib.Path(sys.argv[1])
text = path.read_text()
section = False
for line in text.splitlines():
    if line.strip() == "[workspace.package]":
        section = True
        continue
    if section and line.strip().startswith("["):
        break
    if section:
        match = re.match(r"\s*version\s*=\s*\"([^\"]+)\"", line)
        if match:
            print(match.group(1))
            raise SystemExit(0)
raise SystemExit("workspace.package.version not found")
PY
)"

current_upstream=""
current_kaabil=""
if [[ "${current_version}" =~ ^([0-9]+\.[0-9]+\.[0-9]+)-kaabil\.([0-9]+)$ ]]; then
  current_upstream="${BASH_REMATCH[1]}"
  current_kaabil="${BASH_REMATCH[2]}"
fi

if [[ ${auto} -eq 1 && -n "${current_upstream}" ]]; then
  version_cmp="$(python3 - "${current_upstream}" "${upstream}" <<'PY'
import sys
a = tuple(int(x) for x in sys.argv[1].split("."))
b = tuple(int(x) for x in sys.argv[2].split("."))
print((a > b) - (a < b))
PY
)"
  if [[ "${version_cmp}" -gt 0 ]]; then
    echo "Current upstream ${current_upstream} is newer than npm ${upstream}; skipping."
    exit 0
  fi
fi

if [[ -z "${kaabil}" ]]; then
  if [[ -n "${current_upstream}" && "${current_upstream}" == "${upstream}" ]]; then
    kaabil=$((current_kaabil + 1))
  else
    kaabil=0
  fi
fi

if [[ ! "${kaabil}" =~ ^[0-9]+$ ]]; then
  echo "Invalid kaabil suffix: ${kaabil}" >&2
  exit 1
fi

new_version="${upstream}-kaabil.${kaabil}"

python3 - "${cargo_toml}" "${new_version}" <<'PY'
import pathlib
import re
import sys
path = pathlib.Path(sys.argv[1])
new_version = sys.argv[2]
text = path.read_text().splitlines()
section = False
updated = False
for idx, line in enumerate(text):
    if line.strip() == "[workspace.package]":
        section = True
        continue
    if section and line.strip().startswith("["):
        break
    if section:
        if re.match(r"\s*version\s*=\s*\"([^\"]+)\"", line):
            text[idx] = re.sub(r"\"[^\"]+\"", f"\"{new_version}\"", line, count=1)
            updated = True
            break
if not updated:
    raise SystemExit("workspace.package.version not found")
path.write_text("\n".join(text) + "\n")
PY

echo "Updated workspace version: ${current_version} -> ${new_version}"
