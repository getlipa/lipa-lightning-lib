FROM rust:latest as builder

WORKDIR /usr/src
ARG checkout=ccd1c465be6982102b6e82a12ac1f4e79ca19fa2
RUN git clone --branch main https://github.com/lightningdevkit/rapid-gossip-sync-server.git
WORKDIR /usr/src/rapid-gossip-sync-server
RUN git checkout $checkout

RUN sed -i 's/SNAPSHOT_CALCULATION_INTERVAL: u32 = 3600 \* 24;/SNAPSHOT_CALCULATION_INTERVAL: u32 = 20;/g' /usr/src/rapid-gossip-sync-server/src/config.rs

RUN cargo install --path .

FROM nginx:1.23.2 as final

COPY --from=builder /usr/local/cargo/bin/rapid-gossip-sync-server /
COPY ./nginx.conf /etc/nginx/nginx.conf
RUN sed -i '46 i ./rapid-gossip-sync-server &' ./docker-entrypoint.sh
