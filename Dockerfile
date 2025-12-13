FROM debian:bullseye-slim

WORKDIR /app

# Extract release tarball from the build context into the image.
COPY public/distribution/releases/linux-amd64/esnode-core-0.1.0-linux-amd64.tar.gz /tmp/esnode-core.tar.gz
RUN tar -xzf /tmp/esnode-core.tar.gz -C /app && \
    rm /tmp/esnode-core.tar.gz && \
    groupadd -r esnode && useradd -r -g esnode esnode && \
    chown -R esnode:esnode /app

USER esnode

EXPOSE 9100

ENTRYPOINT ["/app/esnode-core"]
