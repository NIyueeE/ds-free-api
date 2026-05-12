# Docker 一键构建与测试

这套脚本用于本地快速验证：构建前端、用 Docker 中的 Rust 1.95.0 编译后端、生成 Docker 可访问的临时配置、启动测试容器。

## Windows

```powershell
.\scripts\dev-docker-run.ps1 -ConfigPath "D:\codes\free-apis\ds-free-api-v0.2.6-windows-x86_64\config.toml"
```

## macOS / Linux

```bash
chmod +x scripts/dev-docker-run.sh
./scripts/dev-docker-run.sh --config /path/to/config.toml
```

启动后访问：

```text
http://127.0.0.1:22217/health
http://127.0.0.1:22217/admin
```

## 加速源

脚本只在本次执行中注入加速源：

- Bun/npm：`https://registry.npmmirror.com`
- Cargo：`sparse+https://rsproxy.cn/index/`
- Debian apt：`https://mirrors.tuna.tsinghua.edu.cn/debian`

不会修改用户全局 Cargo、Bun、npm 或系统 apt 配置。

## 参数

Windows：

```powershell
.\scripts\dev-docker-run.ps1 `
  -ConfigPath .\config.toml `
  -Port 22217 `
  -ContainerName ds-free-api-test `
  -SkipFrontend `
  -SkipBuild
```

macOS / Linux：

```bash
./scripts/dev-docker-run.sh \
  --config ./config.toml \
  --port 22217 \
  --name ds-free-api-test \
  --skip-frontend \
  --skip-build
```

`--skip-build` / `-SkipBuild` 复用上一次 Docker volume 中的 `target/debug/ds-free-api`。

## 配置处理

Docker 端口映射要求服务监听 `0.0.0.0`。脚本会复制传入的 `config.toml` 到 `target/docker-test-config.toml`，并只替换：

```toml
host = "127.0.0.1"
```

为：

```toml
host = "0.0.0.0"
```

原配置文件不会被修改。

## 容器与缓存

默认容器名：

```text
ds-free-api-test
```

默认 Docker volumes：

```text
ds-free-api-cargo-home
ds-free-api-target
ds-free-api-test-data
```

查看日志：

```bash
docker logs -f ds-free-api-test
```

停止测试容器：

```bash
docker rm -f ds-free-api-test
```

## 常见问题

`/health` 正常但 `/admin` 404：

```text
web/dist 未挂载或未构建 → 运行脚本时不要传 --skip-frontend
```

apt 下载失败：

```text
脚本已设置清华源和 5 次重试；重新执行同一命令即可复用已下载缓存。
```

Cargo 下载慢：

```text
脚本在容器内写入 /usr/local/cargo/config.toml，使用 rsproxy sparse registry。
```
