#!/bin/sh
# Test stub: emits a fixed JSON document, ignoring all arguments.
cat <<'EOF'
{"items":[{"name":"alpha","size":1},{"name":"beta","size":2}]}
EOF
