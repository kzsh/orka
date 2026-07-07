# The base image carries the (slow-changing) apt dependencies and is built
# from Dockerfile.base. Keeping it separate means a --no-cache rebuild here
# (to pull the latest pi) does not re-fetch all the apt packages.
ARG BASE_IMAGE=orka-base:latest
FROM ${BASE_IMAGE}

# Define build arguments for UID and GID with default values

ARG USER_UID=1000
ARG USER_GID=1000
ARG UNAME=appuser
ARG VERSION=latest

# Set to "true" to install the agent-browser extension and its Chromium dependency.
# Omit or set to "false" to skip (saves a large download).
ARG INSTALL_AGENT_BROWSER=false

# Optional: install Chromium's system library dependencies.
# Must run as root before the USER switch.
RUN if [ "$INSTALL_AGENT_BROWSER" = "true" ]; then \
      apt-get update && apt-get install -y --no-install-recommends \
        libatk1.0-0 \
        libatk-bridge2.0-0 \
        libatspi2.0-0 \
        libcairo2 \
        libcups2 \
        libdbus-1-3 \
        libdrm2 \
        libgbm1 \
        libgdk-pixbuf-2.0-0 \
        libglib2.0-0 \
        libgtk-3-0 \
        libnspr4 \
        libnss3 \
        libpango-1.0-0 \
        libpangocairo-1.0-0 \
        libwayland-client0 \
        libx11-6 \
        libx11-xcb1 \
        libxcb1 \
        libxcb-dri3-0 \
        libxcomposite1 \
        libxcursor1 \
        libxdamage1 \
        libxext6 \
        libxfixes3 \
        libxi6 \
        libxkbcommon0 \
        libxrandr2 \
        libxrender1 \
        libxshmfence1 \
        libxss1 \
        libxtst6 \
        libasound2 \
      && rm -rf /var/lib/apt/lists/*; \
    fi

# Create a new group and user with the specified IDs
RUN groupadd -g $USER_GID -o $UNAME && \
    useradd -m -u $USER_UID -g $USER_GID -o -s /bin/bash $UNAME

RUN chown -R "$USER_UID:$USER_GID" /home/$UNAME

# Isolated install root for pi and its bun globals.  Lives outside $HOME so
# that mounted presets (e.g. bun, uv) can never shadow it.
RUN mkdir -p /opt/pi-bun && chown "$USER_UID:$USER_GID" /opt/pi-bun

WORKDIR "/home/$UNAME"

# Switch to the non-root user
USER $UNAME

# Ensure HOME is correct for our user (base image sets it for the bun user)
ENV HOME="/home/$UNAME"

# Point bun's global install to the isolated pi directory so it is never
# reachable from a user-mounted ~/.bun or a preset-injected PATH.
ENV BUN_INSTALL="/opt/pi-bun"
ENV BUN_INSTALL_BIN="/opt/pi-bun/bin"
ENV PATH="/opt/pi-bun/bin:$PATH"

# Install pi before copying entrypoint.sh so that changes to the
# entrypoint script don't bust the expensive bun install cache layer.
RUN bun install --global @earendil-works/pi-coding-agent@${VERSION} && \
    which pi

# Optional: install the agent-browser extension and download Chromium.
# Activated by passing --build-arg INSTALL_AGENT_BROWSER=true.
RUN if [ "$INSTALL_AGENT_BROWSER" = "true" ]; then \
      bun install --global agent-browser && \
      agent-browser install; \
    fi

COPY ./entrypoint.sh /usr/local/bin/entrypoint.sh
ENTRYPOINT ["/usr/local/bin/entrypoint.sh"]
