#!/usr/bin/env bash
# Always launch box Chrome with AT-SPI renderer tree for cu.
# Extra args are passed through. Do not add a second --force-renderer-accessibility.
exec /usr/local/bin/box-chrome --force-renderer-accessibility "$@"
