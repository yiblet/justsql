FROM rust:1.52-slim-buster as builder

WORKDIR /root/justsql
RUN cargo init . --bin
COPY Cargo.toml Cargo.toml
COPY Cargo.lock Cargo.lock
RUN cargo build --locked --release 
RUN rm -r ./target/release/deps/justsql*
COPY src src
RUN cargo build --locked --release

FROM debian:buster-slim
COPY --from=builder /root/justsql/target/release/justsql /usr/bin/justsql
ENTRYPOINT ["/usr/bin/justsql"]
CMD ["help"]
