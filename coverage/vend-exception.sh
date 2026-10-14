#!/usr/bin/env bash
# Approve one test exception: append it to coverage/exceptions/<package>.yaml
# with a signed token. Only a human runs this; an agent asks first and explains why.
#
#   coverage/vend-exception.sh <package> <test> "<reason>"
#
# <test> is the test id as the cluster files show it.
set -euo pipefail
exec python3 -I "$(dirname "$0")/exceptions.py" vend "$@"
