FROM rust:latest as builder

WORKDIR /usr/src
RUN git clone --depth 1 https://github.com/lightningdevkit/rapid-gossip-sync-server/
WORKDIR /usr/src/rapid-gossip-sync-server
RUN git reset --hard f0ef17314840b4e30d08511dfc7773b4ffc8d343
COPY ./config.rs /usr/src/rapid-gossip-sync-server/src/config.rs
RUN sed -i 's/Bitcoin/Regtest/g' /usr/src/rapid-gossip-sync-server/src/lib.rs
RUN cargo install --path .

FROM nginx:1.23.2

COPY --from=builder /usr/local/cargo/bin/rapid-gossip-sync-server /
COPY ./nginx.conf /etc/nginx/nginx.conf
RUN sed -i '46 i ./rapid-gossip-sync-server &' ./docker-entrypoint.sh
