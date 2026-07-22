# ghcr.io/quantaminds/qm — the headless `qm` CLI as a container, for CI/CD
# pipelines and ephemeral runners. Built by .github/workflows/docker.yml from
# the ALREADY-RELEASED (and attestation-verified) binaries — never rebuilt here,
# so the image bit-for-bit ships what the GitHub Release ships.
#
# Base: distroless/cc — glibc + CA certs (rustls-native-roots needs a trust
# store for https backends), no shell, ~24 MB. amd64 gets the static-musl
# binary, arm64 the glibc one; both run on this base.
#
# The container's localhost is NOT the host — to reach an Ollama on the host:
#   docker run --rm --add-host=host.docker.internal:host-gateway \
#     ghcr.io/quantaminds/qm doctor --backend ollama --base http://host.docker.internal:11434
FROM gcr.io/distroless/cc-debian12
ARG TARGETARCH
COPY binaries/${TARGETARCH}/qm /usr/local/bin/qm
ENTRYPOINT ["/usr/local/bin/qm"]
