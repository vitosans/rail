#!/usr/bin/env bash
# Print the notes for release v<version>: its CHANGELOG.md section, then how to pick, check and
# install an archive. Run from the repository root.
#
# usage: scripts/release-notes.sh <version>
set -euo pipefail

if [ "$#" -ne 1 ]; then
  echo "usage: $0 <version>" >&2
  exit 2
fi
version=$1
tag="v${version}"

changes=$(awk -v head="## [${version}]" '
  index($0, head) == 1 { on = 1; next }
  on && /^## \[/ { exit }
  on { print }
' CHANGELOG.md)
if [ -z "${changes//[[:space:]]/}" ]; then
  echo "error: CHANGELOG.md has no section for ${version}" >&2
  exit 1
fi

printf '%s\n\n' "$changes"
cat <<EOF
### Downloads

| Archive | For |
|---|---|
| \`rail-${tag}-x86_64-unknown-linux-gnu.tar.gz\` | Linux x86_64, glibc 2.36 or newer |
| \`rail-${tag}-aarch64-unknown-linux-gnu.tar.gz\` | Linux ARM64, glibc 2.36 or newer |
| \`rail-${tag}-aarch64-apple-darwin.tar.gz\` | macOS 11 or newer on Apple Silicon |
| \`rail-${tag}-x86_64-apple-darwin.tar.gz\` | macOS 11 or newer on Intel |

Each archive holds the \`rail\` binary, the README and \`docs/\`, the changelog, the licences and
\`BUILD-INFO.txt\`. Extract it and put \`rail\` in a directory on your \`PATH\`.

Check an archive against \`SHA256SUMS\`, and check where it was built:

\`\`\`sh
sha256sum -c --ignore-missing SHA256SUMS   # macOS: shasum -a 256 -c --ignore-missing SHA256SUMS
gh attestation verify rail-${tag}-<target>.tar.gz --repo vitosans/rail
\`\`\`

On other systems, build from the tag with Rust 1.88 or newer:

\`\`\`sh
git clone --branch ${tag} --depth 1 https://github.com/vitosans/rail.git
cd rail
cargo install --locked --path crates/rail
\`\`\`
EOF
