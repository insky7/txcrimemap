FROM rust:latest as builder
RUN apt-get update && apt-get install -y musl-tools
RUN rustup target add x86_64-unknown-linux-musl
WORKDIR /app
COPY . .
RUN cargo build --release --target x86_64-unknown-linux-musl

FROM public.ecr.aws/lambda/provided:al2
COPY --from=builder /app/target/x86_64-unknown-linux-musl/release/your-binary /var/runtime/bootstrap
CMD ["/var/runtime/bootstrap"]