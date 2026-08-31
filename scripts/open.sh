#!/bin/sh
# Action launcher: opens the exposé popup entrypoint declared in the manifest.
exec "${HERDR_BIN_PATH:-herdr}" plugin pane open --plugin vjeantet.expose --entrypoint expose
