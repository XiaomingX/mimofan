# Docker

mimofan 提供多架构 Linux 镜像。

## 快速开始

```bash
docker pull ghcr.io/hmbown/mimofan:latest
docker run -it --rm ghcr.io/hmbown/mimofan:latest
```

## 使用 API Key

```bash
docker run -it --rm \
  -e DEEPSEEK_API_KEY=your-key \
  ghcr.io/hmbown/mimofan:latest
```

## 挂载工作区

```bash
docker run -it --rm \
  -v $(pwd):/workspace \
  -e DEEPSEEK_API_KEY=your-key \
  ghcr.io/hmbown/mimofan:latest \
  --workspace /workspace
```

## 架构支持

镜像支持 `linux/amd64` 和 `linux/arm64`。

## 构建镜像

```bash
docker build -t mimofan .
```
