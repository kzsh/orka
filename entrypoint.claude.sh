#!/bin/bash
# Always prepend the isolated claude install dir first so it wins over any
# PATH injected by a --env flag from a preset.
export PATH="/opt/claude-bun/bin:$PATH"

echo "====================="
echo "Claude Code in a container"
echo "====================="
which claude
claude --version
key_status() { [ -n "$1" ] && echo set || echo unset; }
echo "ANTHROPIC_API_KEY=$(key_status "$ANTHROPIC_API_KEY")"
if [[ -n $DEBUG ]]; then
  # Give the user a chance to examine the environment before a screen clear
  echo 'press any key to continue'
  read test
fi

claude "$@"
