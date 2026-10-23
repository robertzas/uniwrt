# The uniwrt image: one static binary on scratch (docs/dev/BUILD.md, Runtime contract). It packages
# the prebuilt dist/uniwrt-<arch> binaries from `just cross` or CI, so no stage runs under emulation.

FROM --platform=$BUILDPLATFORM busybox:1.38.0@sha256:fd7dc98638c8e305f4dc34e979f1c0fdfdcaeb0fbf8fcff77ae834b6da3d7e6e AS prep
ARG TARGETARCH
ARG TARGETVARIANT
COPY dist/ /dist/
# /data is private: it will hold the database and the master key. It is staged inside a parent
# directory because COPY of a directory copies only its contents, which would drop the mode.
RUN mkdir -p /out/state \
 && mkdir -m 0700 /out/state/data \
 && cp /dist/uniwrt-${TARGETARCH}${TARGETVARIANT} /out/uniwrt \
 && chmod 0755 /out/uniwrt

FROM scratch
COPY --from=prep /out/uniwrt /uniwrt
COPY --from=prep --chown=65532:65532 /out/state/ /
USER 65532:65532
ENV UNIWRT_DATA_DIR=/data
VOLUME ["/data"]
EXPOSE 8080/tcp 8443/tcp 5514/tcp 5514/udp
# Probes every second while starting, so the container turns healthy right after the server is up.
HEALTHCHECK --interval=30s --timeout=5s --start-period=30s --start-interval=1s CMD ["/uniwrt", "healthcheck"]
LABEL org.opencontainers.image.title="uniwrt" \
      org.opencontainers.image.description="UniFi-class controller for OpenWrt mesh networks" \
      org.opencontainers.image.source="https://github.com/robertzas/uniwrt" \
      org.opencontainers.image.licenses="MIT OR Apache-2.0"
ENTRYPOINT ["/uniwrt"]
CMD ["serve"]
