# 安装 mimofan

最简安装方式（macOS/Linux）：

```bash
curl -fsSL https://mimofan.net/install.sh | sh
```

---

## 1. 支持的平台

| 平台 | 架构 | npm/pnpm | cargo install | GitHub Release |
|------|------|:--------:|:-------------:|---------------|
| Linux | x64 | ✅ | ✅ | 静态构建 (musl)，无 glibc 依赖 |
| Linux | arm64 | ✅ | ✅ | glibc 构建，需 GLIBC_2.39+ |
| Linux | riscv64 | ✅ | ✅ | glibc 构建，需 GLIBC_2.39+ |
| macOS | x64 / arm64 | ✅ | ✅ | ✅ |
| Windows | x64 | ✅ | ✅ | ✅ |

> arm64/riscv64 在 Ubuntu 22.04 等老系统上可能遇到 `GLIBC_2.39 not found`，需用 cargo 从源码编译。

---

## 2. npm / pnpm 安装（推荐）

```bash
# npm
npm install -g mimofan

# pnpm
pnpm add -g mimofan
```

常用环境变量：

| 变量 | 用途 |
|------|------|
| `MIMOFAN_VERSION` | 指定下载的版本 |
| `MIMOFAN_RELEASE_BASE_URL` | 自定义下载镜像地址 |
| `MIMOFAN_FORCE_DOWNLOAD=1` | 强制重新下载 |
| `MIMOFAN_DISABLE_INSTALL=1` | 跳过 postinstall 下载 |
| `MIMOFAN_OPTIONAL_INSTALL=1` | 下载失败时不报错（CI 用） |

> **国内 npm 慢？** 使用镜像：`npm config set registry https://registry.npmmirror.com`

---

## 3. Cargo 安装

```bash
# 需要 Rust 1.88+
cargo install mimofan-cli --locked
cargo install mimofan --locked
```

> **Linux 需先安装依赖：**
> ```bash
> # Debian/Ubuntu
> sudo apt-get install -y build-essential pkg-config libdbus-1-dev
> ```

### 国内镜像加速

```bash
# rustup 镜像
export RUSTUP_DIST_SERVER=https://rsproxy.cn
export RUSTUP_UPDATE_ROOT=https://rsproxy.cn/rustup
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable

# Cargo 镜像 (~/.cargo/config.toml)
# [source.crates-io]
# replace-with = "tuna"
# [source.tuna]
# registry = "sparse+https://mirrors.tuna.tsinghua.edu.cn/crates.io-index/"
```

---

## 4. 手动下载

从 [Releases](https://github.com/XiaomingX/mimofan/releases) 下载对应平台的两个二进制文件，放到 `PATH` 目录：

```bash
mkdir -p ~/.local/bin
curl -L -o ~/.local/bin/mimofan https://github.com/XiaomingX/mimofan/releases/latest/download/mimofan-linux-x64
curl -L -o ~/.local/bin/mimofan-tui https://github.com/XiaomingX/mimofan/releases/latest/download/mimofan-linux-x64-tui
chmod +x ~/.local/bin/mimofan ~/.local/bin/mimofan-tui
```

> **macOS 提示"无法验证开发者"：** 运行 `xattr -d com.apple.quarantine ~/.local/bin/mimofan ~/.local/bin/mimofan-tui`

---

## 5. Windows 安装

- **Scoop：** `scoop install mimofan`
- **NSIS 安装包：** 从 Releases 下载 `mimofanSetup.exe`，双击安装

---

## 6. 源码编译

```bash
git clone https://github.com/XiaomingX/mimofan.git
cd mimofan
cargo install --path crates/cli --locked
cargo install --path crates/tui --locked
```

---

## 7. 国内常见问题

### npm 下载超时

设置镜像源或使用 cargo 安装。也可设置 `MIMOFAN_RELEASE_BASE_URL` 指向内部镜像。

### `mimofan update` 被墙

通过 CNB 镜像安装：

```bash
cargo install --git https://cnb.cool/mimofan.net/mimofan --tag vX.Y.Z mimofan-cli --locked --force
cargo install --git https://cnb.cool/mimofan.net/mimofan --tag vX.Y.Z mimofan --locked --force
```

### Windows TLS 握手失败

```bash
export RUSTUP_DIST_SERVER=https://rsproxy.cn
export RUSTUP_UPDATE_ROOT=https://rsproxy.cn/rustup
```

### 杀毒软件拦截 cargo build

将项目 `target/` 目录加入杀软白名单，或临时关闭杀软。

---

## 8. 验证安装

```bash
mimofan --version
mimofan doctor    # 检查 API key、provider、运行环境
```
