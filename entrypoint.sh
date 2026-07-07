#!/bin/bash
# Always prepend the isolated pi install dir first so it wins over any
# PATH injected by a --env flag from a preset (e.g. the uv preset).
export PATH="/opt/pi-bun/bin:$PATH"
echo "====================="
echo "Orka: pi"
echo "====================="
which pi
pi --version
key_status() { [ -n "$1" ] && echo set || echo unset; }
echo "ANTHROPIC_API_KEY=$(key_status "$ANTHROPIC_API_KEY")"
echo "OPENAI_API_KEY=$(key_status "$OPENAI_API_KEY")"
echo "OPEN_ROUTER_KEY=$(key_status "$OPEN_ROUTER_KEY")"
if [[ -n $DEBUG ]]; then
  # Give the user a chance to examine the environment before a screen clear
  echo 'press any key to continue'
  read test
fi

pi "$@"
