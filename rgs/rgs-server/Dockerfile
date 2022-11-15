FROM rust:latest as builder

WORKDIR /usr/src
RUN git clone --depth 1 https://github.com/lightningdevkit/rapid-gossip-sync-server/
WORKDIR /usr/src/rapid-gossip-sync-server
RUN git reset --hard f0ef17314840b4e30d08511dfc7773b4ffc8d343

RUN sed -i 's/std::net::SocketAddr;/std::net::{SocketAddr, ToSocketAddrs};/g' /usr/src/rapid-gossip-sync-server/src/config.rs
RUN sed -i '214 i let mut cln_addrs_iter = "rgs-cln:9937".to_socket_addrs().unwrap();' /usr/src/rapid-gossip-sync-server/src/config.rs
RUN sed -i '215 i let cln_addr = cln_addrs_iter.next().unwrap();' /usr/src/rapid-gossip-sync-server/src/config.rs
RUN sed -i 's/(hex_utils::to_compressed_pubkey("035e4ff418fc8b5554c5d9eea66396c227bd429a3251c8cbc711002ba215bfc226").unwrap(), "170.75.163.209:9735".parse().unwrap())/(hex_utils::to_compressed_pubkey("03f3bf54dd54d3cebb21665f8af405261ca8a241938254a46b1ead7b569199f607").unwrap(), cln_addr)/g' /usr/src/rapid-gossip-sync-server/src/config.rs

RUN sed -i 's/Bitcoin/Regtest/g' /usr/src/rapid-gossip-sync-server/src/lib.rs

RUN cargo install --path .

FROM nginx:1.23.2

COPY --from=builder /usr/local/cargo/bin/rapid-gossip-sync-server /
COPY ./nginx.conf /etc/nginx/nginx.conf
RUN sed -i '46 i ./rapid-gossip-sync-server &' ./docker-entrypoint.sh
