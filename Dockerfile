ARG IMAGE=rustlang/rust:nightly

#Builder
FROM $IMAGE as build
WORKDIR /app/src
COPY . .
RUN rustup component add rustfmt
RUN cargo install --path . --root /app/build

# Runner
FROM $IMAGE as runner
WORKDIR /app
COPY --from=build /app/build .

ENTRYPOINT ["/app/bin/backend-api"]
