# The base image carries the (slow-changing) apt dependencies and is built
# from Dockerfile.base. Keeping it separate means a --no-cache rebuild here
# (to pull the latest pi) does not re-fetch all the apt packages.
ARG BASE_IMAGE=pita-base:latest
FROM ${BASE_IMAGE}

# Define build arguments for UID and GID with default values

ARG USER_UID=1000
ARG USER_GID=1000
ARG UNAME=appuser
ARG VERSION=latest

# Create a new group and user with the specified IDs
RUN groupadd -g $USER_GID -o $UNAME && \
    useradd -m -u $USER_UID -g $USER_GID -o -s /bin/bash $UNAME

RUN chown -R "$USER_UID:$USER_GID" /home/$UNAME
WORKDIR "/home/$UNAME"

# Switch to the non-root user
USER $UNAME

# Ensure HOME is correct for our user (base image sets it for the bun user)
ENV HOME="/home/$UNAME"

# Point bun's global install to user-writable locations
ENV BUN_INSTALL="/home/$UNAME/.bun"
ENV BUN_INSTALL_BIN="/home/$UNAME/.bun/bin"
ENV PATH="/home/$UNAME/.bun/bin:$PATH"

COPY ./entrypoint.sh /usr/local/bin/entrypoint.sh
RUN bun install --global @earendil-works/pi-coding-agent@${VERSION} && \
    which pi

ENTRYPOINT ["/usr/local/bin/entrypoint.sh"]
