#! /bin/sh
# Generate `test.iso` and `hash.txt`.
# Requires mkisofs(1) and sha256sum(1).
set -eu

iso_file=test.iso
hash_file=hash.txt

dir=$(dirname "$(realpath "$0")")
cd "$dir"

temp_dir=$(mktemp -d)
clean_up() {
    rm -r "$temp_dir"
}
trap clean_up EXIT

echo 'Hello, world!' > "$temp_dir"/hello.txt
mkisofs -input-charset utf-8 -o "$iso_file" -q "$temp_dir"
sha256sum "$iso_file" > "$hash_file"
