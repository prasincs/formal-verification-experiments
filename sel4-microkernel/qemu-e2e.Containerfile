# Pinned by digest for reproducibility. Digest is the multi-arch manifest list
# for ubuntu:24.04, resolved 2026-07-18. To refresh:
#   curl -sI -H "Authorization: Bearer $(curl -s 'https://auth.docker.io/token?service=registry.docker.io&scope=repository:library/ubuntu:pull' | jq -r .token)" \
#     -H "Accept: application/vnd.oci.image.index.v1+json" \
#     https://registry-1.docker.io/v2/library/ubuntu/manifests/24.04 | grep -i docker-content-digest
FROM ubuntu:24.04@sha256:4fbb8e6a8395de5a7550b33509421a2bafbc0aab6c06ba2cef9ebffbc7092d90

ARG RUST_TOOLCHAIN=nightly-2026-07-02
ARG MICROKIT_VERSION=2.1.0
ARG MICROKIT_SDK_SHA256=faff1b6d6b546cbb0bfea134588499533130d406ae2a5e533e791ddf23ac7599

ENV DEBIAN_FRONTEND=noninteractive
ENV MICROKIT_SDK=/opt/microkit-sdk
ENV RUSTUP_HOME=/opt/rust/rustup
ENV CARGO_HOME=/cargo
ENV PATH=/opt/rust/cargo/bin:${PATH}

RUN apt-get update && apt-get install -y --no-install-recommends \
        bash \
        build-essential \
        ca-certificates \
        curl \
        gcc-aarch64-linux-gnu \
        gcc-riscv64-linux-gnu \
        git \
        ipxe-qemu \
        ipxe-qemu-256k-compat-efi-roms \
        libclang-dev \
        make \
        qemu-efi-aarch64 \
        qemu-system-arm \
        qemu-system-misc \
    && rm -rf /var/lib/apt/lists/*

RUN mkdir -p /opt/rust/cargo /opt/rust/rustup /cargo \
    && curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs -o /tmp/rustup-init.sh \
    && CARGO_HOME=/opt/rust/cargo RUSTUP_HOME=/opt/rust/rustup \
        sh /tmp/rustup-init.sh -y --no-modify-path --profile minimal --default-toolchain "${RUST_TOOLCHAIN}" \
    && CARGO_HOME=/opt/rust/cargo RUSTUP_HOME=/opt/rust/rustup \
        /opt/rust/cargo/bin/rustup component add rust-src --toolchain "${RUST_TOOLCHAIN}" \
    && rm /tmp/rustup-init.sh \
    && chmod -R a+rX /opt/rust \
    && chmod 0777 /cargo

RUN curl -L -o /tmp/microkit-sdk.tar.gz \
        "https://github.com/seL4/microkit/releases/download/${MICROKIT_VERSION}/microkit-sdk-${MICROKIT_VERSION}-linux-x86-64.tar.gz" \
    && echo "${MICROKIT_SDK_SHA256}  /tmp/microkit-sdk.tar.gz" | sha256sum -c - \
    && mkdir -p "${MICROKIT_SDK}" \
    && tar -xzf /tmp/microkit-sdk.tar.gz -C "${MICROKIT_SDK}" --strip-components=1 \
    && rm /tmp/microkit-sdk.tar.gz \
    && chmod -R a+rX "${MICROKIT_SDK}"

WORKDIR /work
