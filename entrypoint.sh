#!/bin/bash
echo "====================="
echo "Pi in a container"
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
