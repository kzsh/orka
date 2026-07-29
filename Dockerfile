# The base image carries the (slow-changing) apt dependencies and is built
# from Dockerfile.base. Keeping it separate means a --no-cache rebuild here
# (to pull the latest pi) does not re-fetch all the apt packages.
ARG BASE_IMAGE=orka-base:latest
FROM ${BASE_IMAGE}

ARG USER_UID=1000
ARG USER_GID=1000
ARG UNAME=appuser
ARG VERSION=latest

# Create a group and user matching the host's uid/gid so volume-mounted paths
# have the correct ownership inside the container.
RUN groupadd -g $USER_GID $UNAME && \
    useradd -m -u $USER_UID -g $USER_GID -s /bin/bash $UNAME

RUN chown -R "$USER_UID:$USER_GID" /home/$UNAME

# Isolated install root for pi and its bun globals.  Lives outside $HOME so
# that mounted presets (e.g. bun, uv) can never shadow it.
# /opt/browser-cache holds the Chromium binary when agent-browser is present
# in the base image.  mkdir -p is a no-op if the directory already exists;
# it also ensures the chown succeeds when a custom base omits agent-browser.
RUN mkdir -p /opt/pi-bun /opt/browser-cache \
    && chown -R "$USER_UID:$USER_GID" /opt/pi-bun /opt/browser-cache

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

COPY ./entrypoint.sh /usr/local/bin/entrypoint.sh
ENTRYPOINT ["/usr/local/bin/entrypoint.sh"]
