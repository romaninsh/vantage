#!/bin/sh
# Test stub: proves the child runs with base_dir as its cwd by reading a
# sibling file via a relative path, then emitting it as a JSON row.
name=$(cat ./sibling.txt)
printf '{"items":[{"name":"%s"}]}\n' "$name"
