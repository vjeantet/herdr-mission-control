#!/bin/sh
# Action launcher: opens the Mission Control popup entrypoint declared in the manifest.
exec "${HERDR_BIN_PATH:-herdr}" plugin pane open --plugin vjeantet.mission-control --entrypoint mission-control
