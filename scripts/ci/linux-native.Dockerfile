# Signed Fedora userspace; installation runs under Docker's ordinary policies.
FROM registry.fedoraproject.org/fedora:44@sha256:e70db1fd517c7d6990715fb200b1aae95ef6a1ef1492dc12c8ca25cc129a5094
SHELL ["/bin/bash", "-euo", "pipefail", "-c"]
RUN source /etc/os-release && test "${ID}" = fedora && test "${VERSION_ID}" = 44 \
    && dnf upgrade -y \
    && ! dnf repolist --enabled | grep -Eiq 'rawhide|updates-testing' \
    && dnf install -y bubblewrap curl dbus-daemon gcc gcc-c++ git gtk3-devel \
       libseccomp make openssl-devel pkgconf-pkg-config shadow-utils tar \
       webkit2gtk4.1-devel xorg-x11-server-Xvfb \
    && dnf upgrade -y --refresh --enablerepo=updates-testing \
       javascriptcoregtk4.1 javascriptcoregtk4.1-devel webkit2gtk4.1 webkit2gtk4.1-devel \
    && dnf clean all \
    && groupadd --gid 10001 native \
    && useradd --uid 10001 --gid 10001 --create-home native \
    && git config --system safe.directory /workspace
USER 10001:10001
WORKDIR /workspace
ENV HOME=/home/native CARGO_HOME=/home/native/cargo CARGO_TARGET_DIR=/home/native/target
ENV RUSTUP_HOME=/opt/rustup RUSTUP_TOOLCHAIN=1.95.0 PATH=/opt/rust-bin:/usr/local/bin:/usr/bin:/bin
CMD ["sleep", "infinity"]
