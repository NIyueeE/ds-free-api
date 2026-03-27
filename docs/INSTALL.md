# 小白安装教程

本教程面向没有任何编程经验的用户，手把手教你从 GitHub 下载安装包，在 Windows / macOS / Linux 上运行 DeepSeek Web API。

**唯一的前置依赖：`uv`**（一个超快的 Python 包管理器）

---

## 第一步：安装 uv

uv 是这个项目的唯一依赖。它是一个 Python 包管理器，比 pip 更快更简单。

[安装 | uv 中文文档](https://uv.doczh.com/getting-started/installation/)

> 点击上面链接, 选择适合你平台的安装方式;
>
> Windows平台如果习惯使用winget也可以通过`winget install -e --id astral-sh.uv`安装. (更推荐)

### 验证安装成功

重新打开终端后，输入：

```bash
uv --version
```

看到类似 `uv 0.x.x` 的版本号就说明安装成功了。

---

## 第二步：下载 DeepSeek Web API

### 方法一：从 GitHub 下载安装包

1. 在浏览器访问项目的 [主页面](https://github.com/NIyueeE/deepseek-web-api), 点击下载ZIP文件

![image-20260327161548910](assets/image-20260327161548910.png)

2. 把下载的文件解压到一个**你找得到的文件夹**（比如桌面）

---

## 第三步：配置账号信息

1. 进入解压后的文件夹，找到 `config.toml.example` 文件
2. **复制** 这个文件，并**重命名** 为 `config.toml`
3. 用任何**文本编辑器**打开 `config.toml`
4. 找到 `[account]` 部分，填入你的 DeepSeek 账号信息：

```toml
[account]
email = "你的邮箱@example.com"        # 填你的 DeepSeek 邮箱
mobile = ""                           # 手机号（二选一, 或者都填上）
area_code = "86"                      # 区号
password = "你的密码"                 # 填你的 DeepSeek 密码
token = ""                            # 留空，系统会自动获取
```

5. **保存文件**

> [!WARNING]
> **安全提示**：如果你只是在本机自己用，可以跳过下面的"配置访问令牌"步骤。如果你要在其他电脑或网络上访问这个 API，建议配置令牌。

### （可选）配置访问令牌

如果你希望只有知道令牌的人才能访问这个 API：

1. 在 `config.toml` 中找到 `[auth]` 部分
2. 把 `tokens = []` 改成 `tokens = ["你的随机令牌"]`，比如：

```toml
[auth]
tokens = ["sk-my-secret-token-12345"]
```

3. (可选) 修改监听访问

~~~toml
[server]
host = "0.0.0.0"  # 或者其他
~~~

4. 保存文件

---

## 第四步：运行服务

1. 打开终端，进入解压后的文件夹：

> [!NOTE]
>
> Windows确保你有[Windows Terminal](https://apps.microsoft.com/detail/9n0dx20hk701?launch=true&mode=full&hl=zh-cn&gl=cn&ocid=bingwebsearch)

```bash
# Windows 用 cd
cd 路径\到\deepseek-web-api

# macOS / Linux 用 cd
cd 路径/deepseek-web-api
```

2. 输入以下命令启动服务：

```bash
uv run python main.py
```

3. 第一次运行会**自动安装依赖**（curl-cffi、fastapi 等），稍等片刻
4. 看到类似下面的输出就说明成功了：

```
INFO:     Application startup complete.
```

5. 现在可以打开浏览器访问 `http://127.0.0.1:5001` 可以看到如下就是没问题

~~~
{"status":"ok","service":"deepseek-web-api"}
~~~

> **注意**：关闭终端窗口会停止服务。如果要后台运行，可以按 `Ctrl + C` 停止，然后终端中用 
>
> macOS/Linux: `nohup uv run python main.py &`
>
> Windows: `Start-Process -WindowStyle Hidden "uv" -ArgumentList "run python main.py"`

---

## 第五步：接入支持Openai协议的客户端

服务运行后，可以用浏览器测试：

- baseurl: `http://127.0.0.1:5001/v1` (或者`http://127.0.0.1:5001/v1/chat/completions`)
- api-token: 如果你配置了令牌, 见你创建的`config.toml`

其余的内容可以见 [README.中文.md](..\README.中文.md) 

---

## 常见问题

### Q: 提示"找不到 uv"？

**Windows**：优先使用`winget`方法安装

**macOS/Linux**：把 `~/.local/bin` 加入 PATH。编辑 `~/.bashrc` 或 `~/.zshrc`，添加一行：
```bash
export PATH="$HOME/.local/bin:$PATH"
```
然后 `source ~/.bashrc` 或重启终端。

### Q: 提示"连接失败"？

确保服务正在运行，不要关闭运行服务的终端窗口。

### Q: 提示"认证失败"？

检查 `config.toml` 中的邮箱和密码是否正确。如果是 DeepSeek 手机号登录，填写 `mobile` 而非 `email`。

### Q: 如何停止服务？

在运行服务的终端窗口按 `Ctrl + C`。

### Q: 如何后台运行？

**macOS / Linux**：
```bash
nohup uv run python main.py > output.log 2>&1 &
```

**Windows**：不太推荐后台运行，可以创建一个批处理文件 `start.bat`：
```batch
@echo off
cd /d "%~dp0"
start /min uv run python main.py
```

### Q: 端口被占用？

修改 `config.toml` 中的端口：
```toml
[server]
port = 8080  # 改成其他端口
```

然后访问 `http://127.0.0.1:8080`

---

## 下一步

- 查看 [API 文档](./v0_API.md) 了解所有接口
- 查看 [核心模块文档](./CORE.md) 了解技术细节
- 以后成熟后会支持Claude协议, 以及更加简单的安装方法