#!/bin/bash
echo "====================="
echo "Pi in a container"
echo "====================="
which pi
pi --version
echo "ANTHROPIC_API_KEY=$(echo "$ANTHROPIC_API_KEY" | awk '{ print substr($0, 1, 20) }' )..."
echo "OPENAI_API_KEY=$(echo "$OPENAI_API_KEY" | awk '{ print substr($0, 1, 20) }' )..."
echo "OPEN_ROUTER_KEY=$(echo "$OPEN_ROUTER_KEY" | awk '{ print substr($0, 1, 20) }' )..."
if [[ -n $DEBUG ]]; then
  # Give the user a chance to examine the environment before a screen clear
  echo 'press any key to continue'
  read test
fi

pi "$@"
