#!/usr/bin/env bash
# One octocov report per crate, so a PR that tested two crates compares those
# two against main's latest numbers for the same crates (octocov's monorepo
# key, owner/repo/<crate>), not against a whole-workspace percentage.
#
#   coverage/ci-octocov.sh <lcov.info> <measured json from script/ci-changes>
#
# Needs octocov on PATH and GITHUB_TOKEN. Writes the job summary; on a push
# it stores each crate's report as the artifact later PRs compare with.
set -euo pipefail
lcov=$1
measured=$2
work=$(mktemp -d)

jq -c '.[]' <<<"$measured" | while read -r crate; do
  name=$(jq -r .name <<<"$crate")
  dir=$(jq -r .dir <<<"$crate")
  # Keep the records of this crate's sources: SF:<abs path>/<dir>/src/... up to end_of_record.
  awk -v d="/$dir/src/" '/^SF:/{keep = index($0, d) > 0} keep' "$lcov" > "$work/$name.info"
  if [[ ! -s "$work/$name.info" ]]; then
    echo "$name: no coverage records" >&2
    continue
  fi
  cat > "$work/$name.yml" <<EOF
repository: ${GITHUB_REPOSITORY}/${name}
coverage:
  paths:
    - $work/$name.info
codeToTestRatio:
  code:
    - "$dir/src/**/*.rs"
  test:
    - "$dir/tests/**/*.rs"
diff:
  datastores:
    - artifact://${GITHUB_REPOSITORY}
comment:
  if: false
summary:
  if: true
report:
  if: github.event_name == 'push'
  datastores:
    - artifact://${GITHUB_REPOSITORY}
EOF
  octocov --config "$work/$name.yml"
done
