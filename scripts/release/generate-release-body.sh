#!/usr/bin/env bash
# Generate the GitHub Release body for a tag.
#
# Usage: generate-release-body.sh <vX.Y.Z> [path/to/CHANGELOG.md]
#
# The install/verify sections are static; the release notes and contributor
# credits come from the CHANGELOG section for the version, so they can never
# drift the way a hand-edited workflow body does.
set -euo pipefail

tag="${1:?usage: $0 <vX.Y.Z> [CHANGELOG.md]}"
changelog="${2:-CHANGELOG.md}"
version="${tag#v}"

section="$(awk -v version="${version}" '
  index($0, "## [" version "]") == 1 { in_section = 1; next }
  in_section && /^## \[/ { exit }
  in_section { print }
' "${changelog}")"

cat <<EOF
> **Mimofan** is the canonical project, command, bun package, and
> release-asset name. The legacy npm package \`deepseek-tui\` is
> deprecated and receives no further releases. Users coming from
> v0.8.x legacy \`deepseek\` / \`deepseek-tui\` names should migrate
> with \`docs/REBRAND.md\`.

## Install

### Recommended — bun (one command)

\`\`\`bash
bun add -g mimofan
\`\`\`

The wrapper downloads the single \`mimofan\` binary from this Release.

### Docker / GHCR

\`\`\`bash
docker run --rm -it \\
  -e DEEPSEEK_API_KEY="\$DEEPSEEK_API_KEY" \\
  -v ~/.deepseek:/home/mimofan/.deepseek \\
  ghcr.io/hmbown/mimofan:${tag}
\`\`\`

The image ships the \`mimofan\` binary. The \`latest\` tag is also updated on release.

### Cargo (Linux / macOS)

\`\`\`bash
cargo install mimofan --locked
\`\`\`

### Manual download — platform archives (recommended)

Each archive below contains the \`mimofan\` binary plus an install script:

| Platform | Archive | Install script |
|---|---|---|
| Linux x64 | \`mimofan-linux-x64.tar.gz\` | \`install.sh\` |
| Linux ARM64 | \`mimofan-linux-arm64.tar.gz\` | \`install.sh\` |
| macOS x64 | \`mimofan-macos-x64.tar.gz\` | \`install.sh\` |
| macOS ARM | \`mimofan-macos-arm64.tar.gz\` | \`install.sh\` |
| Windows x64 | \`mimofan-windows-x64.zip\` | (bundled \`mimofan.exe\`) |

**Unix (Linux / macOS):**
\`\`\`bash
tar xzf mimofan-<platform>.tar.gz
cd mimofan-<platform>
./install.sh
\`\`\`

**Windows:**
- Extract \`mimofan-windows-x64.zip\`
- Move \`mimofan.exe\` onto your PATH (e.g. \`%USERPROFILE%\\bin\`)
- Add that directory to your PATH

Each platform also has a **bare, unarchived** binary attached below (\`mimofan-<platform>\`) — this is what the bun wrapper and the in-app \`mimofan update\` download, whereas the \`.tar.gz\` / \`.zip\` archives above are the recommended manual download and additionally bundle an install script. The legacy npm package \`deepseek-tui\` is deprecated and is not republished. For migration from v0.8.x legacy binary names, see \`docs/REBRAND.md\`.

### Verify (recommended)

Download the checksum manifests from this Release and verify:

\`\`\`bash
# Linux — archive bundles
sha256sum -c mimofan-bundles-sha256.txt

# Linux — individual binaries
sha256sum -c mimofan-artifacts-sha256.txt

# macOS
shasum -a 256 -c mimofan-bundles-sha256.txt
shasum -a 256 -c mimofan-artifacts-sha256.txt
\`\`\`

## What's in ${tag}
EOF

if [[ -n "${section}" ]]; then
  printf '%s\n' "${section}"
else
  printf '%s\n' "See the changelog link below for this release's notes."
fi

cat <<EOF

Contributor credits for this release live in the changelog entry above —
thank you to everyone whose reports, PRs, reviews, and reproductions shaped it.

See [CHANGELOG.md](https://github.com/XiaomingX/mimofan/blob/main/CHANGELOG.md) for full notes and [docs/CHANGELOG_ARCHIVE.md](https://github.com/XiaomingX/mimofan/blob/main/docs/CHANGELOG_ARCHIVE.md) for older releases.
EOF
