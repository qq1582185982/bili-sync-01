# 使用多阶段构建
# 第一阶段：Rust编译环境
FROM rustlang/rust:nightly-alpine AS builder

WORKDIR /app

# 配置中国区镜像源（这一层会被缓存）
RUN mkdir -p /usr/local/cargo && \
    echo '[source.crates-io]' > /usr/local/cargo/config && \
    echo 'replace-with = "ustc"' >> /usr/local/cargo/config && \
    echo '[source.ustc]' >> /usr/local/cargo/config && \
    echo 'registry = "https://mirrors.ustc.edu.cn/crates.io-index"' >> /usr/local/cargo/config

# 安装编译依赖（这一层会被缓存，除非基础镜像更新）
RUN apk update && apk add --no-cache \
    build-base \
    musl-dev \
    openssl-dev \
    ca-certificates \
    tzdata \
    git \
    sqlite-dev

# 只复制依赖配置文件（优化：减少不必要的文件复制）
COPY Cargo.toml Cargo.lock ./

# 创建一个空的主程序，预先构建依赖（这一层只有在 Cargo.toml 或 Cargo.lock 变化时才会重新构建）
RUN mkdir -p crates/bili_sync/src && \
    echo "fn main() {}" > crates/bili_sync/src/main.rs && \
    printf '[package]\nname = "bili_sync"\nversion = "0.1.0"\nedition = "2021"\n' > crates/bili_sync/Cargo.toml && \
    cargo build --release && \
    rm -rf crates target/release/deps/bili_sync*

# 复制其他配置文件（分离复制，减少缓存失效）
COPY rustfmt.toml ./

# 复制 crates 目录（如果 crates 内容变化，只会影响这一层之后的缓存）
COPY crates ./crates/

# 复制 web 目录（如果 web 内容变化，只会影响这一层之后的缓存）
COPY web ./web/

# 编译项目
RUN cargo build --release && \
    strip target/release/bili-sync-rs

# 第二阶段：运行环境
FROM alpine:latest

WORKDIR /app

# 安装运行时依赖（这一层会被缓存）
RUN apk update && apk add --no-cache \
    ca-certificates \
    tzdata \
    ffmpeg \
    sqlite-libs

# 从构建阶段复制编译好的二进制文件
COPY --from=builder /app/target/release/bili-sync-rs /app/bili-sync-rs

# 设置权限
RUN chmod +x /app/bili-sync-rs

# 设置环境变量
ENV LANG=zh_CN.UTF-8 \
    TZ=Asia/Shanghai \
    HOME=/app \
    RUST_BACKTRACE=1 \
    RUST_LOG=None,bili_sync=info

# 指定入口点
ENTRYPOINT [ "/app/bili-sync-rs" ]

# 定义数据卷，用于持久化配置
VOLUME [ "/app/.config/bili-sync" ]

# 暴露Web界面端口（如果有的话）
EXPOSE 8080

# 健康检查
HEALTHCHECK --interval=30s --timeout=30s --start-period=5s --retries=3 \
    CMD [ "/app/bili-sync-rs", "--health-check" ] || exit 1

# 元数据标签
LABEL maintainer="amtoaer <amtoaer@gmail.com>" \
      description="bili-sync - 由 Rust & Tokio 驱动的哔哩哔哩同步工具" \
      version="2.5.1"

