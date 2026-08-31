# Shared bats helpers for this repo's test suite.
#
# repo_root resolves the repository root from this file's own location, so
# tests work regardless of the working directory bats was invoked from.
repo_root() {
  cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd
}
