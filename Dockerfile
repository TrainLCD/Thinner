FROM rust:1 
WORKDIR /app
RUN apt-get update && \
    apt-get install -y protobuf-compiler libprotobuf-dev && \
    rm -rf /var/lib/apt/lists/*
COPY . .
RUN cargo install --path .

ENV PORT 3000

EXPOSE $PORT

CMD ["thinner"]