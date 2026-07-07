#!/bin/bash
echo "====================="
echo "Orka: codex"
echo "====================="
which codex
codex --version
key_status() { [ -n "$1" ] && echo set || echo unset; }
echo "OPENAI_API_KEY=$(key_status "$OPENAI_API_KEY")"
if [[ -n $DEBUG ]]; then
  echo 'press any key to continue'
  read test
fi

codex "$@"
