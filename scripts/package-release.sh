#!/usr/bin/env bash
# Package a release build of rail into rail-v<version>-<target>.tar.gz: the binary, the README,
# the docs, the changelog, both licences and BUILD-INFO.txt. Run from the repository root after
# `cargo build --release --locked --workspace` on the target itself.
#
# usage: scripts/package-release.sh <target> <out-dir>
#
# The version comes from the rail crate. On a tag push (GITHUB_REF_TYPE=tag) the tag must be
# v<version>; any other build records that it was not tagged.
set -euo pipefail

if [ "$#" -ne 2 ]; then
  echo "usage: $0 <target> <out-dir>" >&2
  exit 2
fi
target=$1
out_dir=$2

version=$(cargo pkgid -p rail | sed 's/.*[#@]//')
if [ "${GITHUB_REF_TYPE:-}" = "tag" ]; then
  tag=$GITHUB_REF_NAME
  if [ "$tag" != "v${version}" ]; then
    echo "error: tag ${tag} does not match the crate version ${version}" >&2
    exit 1
  fi
else
  tag="none (untagged build)"
fi

binary=target/release/rail
name="rail-v${version}-${target}"
stage="${out_dir}/${name}"

if [ ! -x "$binary" ]; then
  echo "error: ${binary} is missing; build the release first" >&2
  exit 1
fi

# The binary must say the version the archive is named for.
reported=$("$binary" --version)
if [ "$reported" != "rail ${version}" ]; then
  echo "error: ${binary} reports '${reported}', expected 'rail ${version}'" >&2
  exit 1
fi

# What the binary needs from the system it runs on.
runtime_requirement() {
  case "$target" in
    *-linux-gnu)
      local glibc
      glibc=$(objdump -T "$binary" | grep -o 'GLIBC_[0-9.]*' | sed 's/GLIBC_//' | sort -uV | tail -1)
      echo "Highest required glibc symbol: ${glibc}"
      ;;
    *-apple-darwin)
      local minos
      minos=$(otool -l "$binary" | awk '/LC_BUILD_VERSION/ {found=1} found && $1 == "minos" {print $2; exit}')
      echo "Minimum macOS: ${minos}"
      ;;
    *)
      echo "Runtime: not recorded for ${target}"
      ;;
  esac
}

mkdir -p "$stage"
cp "$binary" README.md CHANGELOG.md LICENSE-APACHE LICENSE-MIT "$stage/"
cp -R docs "$stage/"

{
  echo "Rail ${version}"
  echo "Tag: ${tag}"
  echo "Source commit: $(git rev-parse HEAD)"
  echo "Source tree: $(git rev-parse 'HEAD^{tree}')"
  echo "Target: ${target}"
  echo "Compiler: $(rustc --version)"
  echo "Build environment: ${BUILD_ENVIRONMENT:-local}"
  echo "Build command: cargo build --release --locked --workspace"
  if [ -n "${GITHUB_RUN_ID:-}" ]; then
    echo "Workflow run: ${GITHUB_SERVER_URL}/${GITHUB_REPOSITORY}/actions/runs/${GITHUB_RUN_ID}"
  fi
  runtime_requirement
  echo "Test features: disabled (normal release workspace build)"
} > "${stage}/BUILD-INFO.txt"

tar -czf "${out_dir}/${name}.tar.gz" -C "$out_dir" "$name"
rm -rf "$stage"
echo "${out_dir}/${name}.tar.gz"
