#!/usr/bin/env bash
set -euo pipefail

# These are the two published Git blobs from one-covenant/basilica at the
# imported source commit. Keep their hashes independent of GitHub's transport.
SOURCE_COMMIT=e40e11f96712806644a586a9ee8895548efdebd9
SOURCE_BASE="https://raw.githubusercontent.com/one-covenant/basilica/$SOURCE_COMMIT/bin"
EXECUTOR_SHA256=056922331186a08fd9a644ce8cbd0b0455accd73bc23dedd740109415ce6968a
VALIDATOR_SHA256=391d6c5c673276e428ee5d9531a8c1aa7e306944a2eeb2102a5d42e548402eab

destination=""
source_dir=""
custom=false

while (($#)); do
    case "$1" in
        --destination) destination="${2:?missing destination}"; shift 2 ;;
        --source-dir) source_dir="${2:?missing source directory}"; shift 2 ;;
        --custom) custom=true; shift ;;
        *) echo "Unknown option: $1" >&2; exit 2 ;;
    esac
done

if [[ -z "$destination" || ( "$custom" == true && -z "$source_dir" ) ]]; then
    echo "Usage: $0 --destination DIR [--source-dir DIR] [--custom]" >&2
    exit 2
fi

if [[ -n "$source_dir" && ! -d "$source_dir" ]]; then
    echo "Veritas source directory does not exist: $source_dir" >&2
    exit 1
fi

hash_file() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | cut -d ' ' -f 1
    else
        shasum -a 256 "$1" | cut -d ' ' -f 1
    fi
}

verify_elf_amd64() {
    local magic machine size
    [[ -s "$1" ]] || { echo "Missing or empty Veritas binary: $1" >&2; return 1; }
    size="$(wc -c < "$1" | tr -d ' ')"
    magic="$(head -c 4 "$1" | od -An -t x1 | tr -d ' \n')"
    machine="$(dd if="$1" bs=1 skip=18 count=2 2>/dev/null | od -An -t x1 | tr -d ' \n')"
    if [[ "$size" -lt 65536 || "$magic" != 7f454c46 || "$machine" != 3e00 ]]; then
        echo "Veritas binary is not a complete x86-64 ELF file: $1" >&2
        return 1
    fi
}

mkdir -p "$destination"
staging="$(mktemp -d "$destination/.veritas-stage.XXXXXX")"
trap 'rm -rf "$staging"' EXIT

stage_one() {
    local name="$1" expected="$2" source="$3" staged
    staged="$staging/$name"
    if [[ -n "$source_dir" ]]; then
        if [[ ! -f "$source" ]]; then
            echo "Missing Veritas binary: $source" >&2
            return 1
        fi
        cp "$source" "$staged"
    else
        curl --fail --location --silent --show-error --proto '=https' --tlsv1.2 \
            --retry 3 --connect-timeout 15 --output "$staged" \
            "$SOURCE_BASE/$name/$name"
    fi

    verify_elf_amd64 "$staged"
    if [[ "$custom" != true && "$(hash_file "$staged")" != "$expected" ]]; then
        echo "SHA-256 mismatch for $name from imported public source" >&2
        return 1
    fi
    chmod 755 "$staged"
}

executor_source=""
validator_source=""
if [[ -n "$source_dir" ]]; then
    executor_source="$source_dir/executor-binary/executor-binary"
    validator_source="$source_dir/validator-binary/validator-binary"
fi

stage_one executor-binary "$EXECUTOR_SHA256" "$executor_source"
stage_one validator-binary "$VALIDATOR_SHA256" "$validator_source"

for name in executor-binary validator-binary; do
    mkdir -p "$destination/$name"
    mv -f "$staging/$name" "$destination/$name/$name"
done
echo "Staged Veritas binaries in $destination"
