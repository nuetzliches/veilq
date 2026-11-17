# What a lane of veilq runs in on a node of the devstack pool (`[lanes]
# dockerfile` in devstack.toml): the toolchains its services build with, as
# CI has them (.github/workflows/ci.yml): Rust (the workspace's
# `rust-version`) and Node 24. The node builds this image from the lane's
# source and supplies devstack itself. Not used for releases.
#
# The models (NER, OCR, PDFium) are not in it: `veilq ui` in such a lane
# runs with the patterns, as a machine without `veilq models fetch` does.

FROM rust:1.98-bookworm

COPY --from=node:24-bookworm-slim /usr/local/bin/node /usr/local/bin/node
COPY --from=node:24-bookworm-slim /usr/local/lib/node_modules /usr/local/lib/node_modules
RUN ln -s ../lib/node_modules/npm/bin/npm-cli.js /usr/local/bin/npm \
 && ln -s ../lib/node_modules/npm/bin/npx-cli.js /usr/local/bin/npx \
 && node --version && npm --version && cargo --version
