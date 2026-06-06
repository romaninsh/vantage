#!/bin/sh
# Test stub: echoes its arguments back as a JSON array of {"arg": "..."}.
printf '['
first=1
for a in "$@"; do
  if [ "$first" -eq 0 ]; then printf ','; fi
  first=0
  printf '{"arg":"%s"}' "$a"
done
printf ']'
