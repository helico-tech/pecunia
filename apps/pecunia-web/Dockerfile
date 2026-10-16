# Pecunia in the browser, self-hosted: the static web app served by nginx (unprivileged, port
# 8080). The app keeps no data on the server; every book stays a file on the visitor's disk.
#
#   docker build -f apps/pecunia-web/Dockerfile -t pecunia-web .
#   docker run --rm -p 8080:8080 pecunia-web        # then open http://localhost:8080
#
# The Rust stage runs on the build machine's own platform: its output (wasm, JS, HTML) is the same
# for every CPU, so building the arm64 image needs no emulation.

FROM --platform=$BUILDPLATFORM rust:1-bookworm AS build
RUN rustup target add wasm32-unknown-unknown
WORKDIR /src
# The wasm-bindgen CLI must match the library version in Cargo.lock.
COPY Cargo.lock ./
RUN version=$(awk '/^name = "wasm-bindgen"$/ { getline; gsub(/version = |"/, ""); print }' Cargo.lock) \
 && cargo install wasm-bindgen-cli --version "$version" --locked
COPY . .
RUN cargo xtask web

FROM nginxinc/nginx-unprivileged:1.27-alpine
COPY apps/pecunia-web/nginx.conf /etc/nginx/conf.d/default.conf
COPY --from=build /src/target/web /usr/share/nginx/html
EXPOSE 8080
