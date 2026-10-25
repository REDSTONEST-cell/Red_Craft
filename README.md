# Red Craft

一个轻量、快速的 Minecraft Java 版启动器。Tauri 2 + Rust + Svelte 5，简洁报刊风界面。

主要面向 Windows，同时支持 macOS 与 Linux。

## 当前进度

| 阶段 | 内容 | 状态 |
| --- | --- | --- |
| 1 | 项目骨架：Tauri 2 + Svelte 5 + launcher-core 工作区，command / event 打通 | ✅ |
| 2 | 版本列表、原版下载与校验、离线登录、启动原版 | ✅ |
| 3 | Java 检测与自动下载（Mojang Java runtime） | ✅ |
| 4 | 微软正版登录、外置登录（authlib-injector） | ✅ |
| 5 | Fabric / Quilt / Forge / NeoForge | ✅ |
| 6 | Mod / 材质包 / 光影包 / 整合包下载（Modrinth、CurseForge）、前置依赖解析、已安装 Mod 管理 | ✅ |
| 7 | 界面打磨与动效（含深色主题） | ✅ |
| 8 | 打包（NSIS / MSI） | CI 已配置 |

已实现的功能：

- **版本管理**：官方版本清单，按正式版 / 快照 / 远古版筛选和搜索（虚拟滚动）；下载版本 JSON、客户端 jar、libraries（按 rules 判断系统与架构）、natives（兼容旧版 classifiers 写法与 1.19+ 新写法，按架构筛选）、资源索引与资源文件、日志配置。所有文件 SHA1 校验，已存在且校验通过的跳过。
- **下载器**：tokio + reqwest + Semaphore 并发（默认 64，可调），失败重试与下载源切换（自动 / 官方 / BMCLAPI），总进度与速度实时推送。
- **版本隔离**：与 PCL 相同的五档策略（关闭 / 隔离可安装 Mod 的版本 / 隔离非正式版 / 两者都隔离 / 全部隔离），每个实例可单独覆盖。
- **便携模式**：启动器所在目录存在 `.minecraft` 时自动使用它，配置也保存在其中。
- **Java**：扫描 JAVA_HOME、PATH、Windows 注册表、常见安装目录、官方启动器自带的 runtime；按版本 JSON 的 `javaVersion` 自动选择，没有合适的就从 Mojang Java runtime 清单下载（目录布局与官方启动器一致，可互相复用）。
- **模组加载器**：新建实例时可选 Fabric / Quilt / Forge / NeoForge，只列出支持所选游戏版本的加载器版本；Fabric / Quilt 直接使用官方 profile，Forge / NeoForge 先用启动器（支持镜像）预下载依赖库，再以无界面方式运行官方安装器；1.12.2 及更早的 Forge 直接解析安装配置，不运行安装器。加载器版本 JSON 通过 inheritsFrom 继承原版。
- **资源下载**：Mod、材质包、光影包，数据源可选 Modrinth 或 CurseForge，按实例的游戏版本与加载器筛选（虚拟滚动、滚动加载）；只列出兼容的版本；安装前自动递归解析必需前置（处理循环依赖、去重、已安装的跳过、只给项目没给版本时选兼容的最新版），可选依赖供勾选，已安装的冲突项给出警告；更新时自动替换旧文件。Modrinth 走 MCIM 镜像，CurseForge 没有 API Key 时通过 MCIM 镜像访问。新建 Fabric / Quilt 实例时自动安装兼容的最新 Fabric API / QSL。
- **整合包**：支持 Modrinth（.mrpack）与 CurseForge 整合包，可在资源页直接安装或从本地导入；自动安装对应的游戏版本与加载器（简写的 Forge 版本号会换成完整版本），下载全部文件（CurseForge 按项目类型放入 mods / resourcepacks / shaderpacks），解压 overrides，并作为开启版本隔离的独立实例。
- **游戏日志**：原版日志配置输出的 log4j XML 会被还原成普通文本行（含多行消息与异常堆栈）。
- **已安装 Mod 管理**：读取 jar 中的 `fabric.mod.json` / `quilt.mod.json` / `mods.toml` / `neoforge.mods.toml` / `mcmod.info` 显示名称、版本、作者与图标；启用 / 禁用（改后缀 `.disabled`）、删除、检查更新（Modrinth 用文件 SHA1 调 `/version_files/update`，从 CurseForge 安装的按安装记录查询）、单个或全部更新（禁用的 Mod 更新后仍保持禁用）。
- **离线账号头像**：离线账号与某个正版账号同名时，头像使用该正版账号的皮肤。
- **高级选项**（设置页，可折叠）：Java 虚拟机参数、游戏参数、启动前执行命令（支持占位符，可选择等待完成）、内存回收器（自动 / G1 / 分代 ZGC / 不指定）、使用高性能显卡（Windows）。
- **默认中文**：实例第一次启动时自动把游戏语言设为简体中文（可在设置中关闭）。
- **离线登录**：UUID 为 `OfflinePlayer:<name>` 的 MD5（UUID v3），与服务端一致。
- **微软正版登录**：点击后自动打开浏览器，确认后自动完成（授权码 + PKCE，本机回调）；也可改用验证码登录。之后依次 Xbox Live → XSTS → Minecraft，检查是否拥有游戏并获取档案；未成年需家长同意、没有 Xbox 档案、没买游戏、应用未通过审核等情况都有中文提示；令牌存系统密钥库，过期自动刷新；显示正版皮肤头像。
- **外置登录**：支持 LittleSkin 等 authlib-injector 认证服务器。输入服务器地址自动识别（ALI），账号密码只用于登录、不保存；多角色时可选择角色；令牌保存在系统密钥库，启动前校验并自动刷新；启动时自动下载并校验 authlib-injector（BMCLAPI 镜像），注入 `-javaagent` 与预取的服务器元数据。
- **从其他启动器导入**：识别官方启动器、Prism Launcher、MultiMC、HMCL（全局账号），或手动选择 HMCL / PCL / .minecraft 文件夹；识别实例的游戏版本（含被重命名的版本）、加载器、Mod 与存档数量。导入时重新安装相同的版本与加载器（强制版本隔离），再复制用户数据，原文件不改动；可同时导入离线账号。
- **OptiFine 与光影加载器**：新建实例时可选 OptiFine（BMCLAPI；原版用 OptiFine 自带补丁程序 + LaunchWrapper 安装，Forge 放进 mods 并自动选择推荐的 Forge 版本），或 Iris（Fabric / Quilt / NeoForge）/ Oculus（Forge）及其前置。
- **自定义背景**：在「设置 → 外观」导入本地图片（JPG / PNG / WebP）或视频（MP4 / WebM），或扫描 Steam 中 Wallpaper Engine 的视频壁纸；可调亮度、透明度、模糊与遮罩，预览后保存；启动器隐藏或最小化时视频自动暂停。（感谢 @Windshakes 贡献）
- **界面风格**：在「设置 → 外观」中可选形状（利落：直角硬朗的报刊风；圆润：圆角与柔和阴影，类似 PCL2）与布局（标准；极简：去掉报头和装饰，只突出主要内容，字号更大）。
- **启动**：同时支持新版 `arguments` 与旧版 `minecraftArguments`；处理 `legacy` / `pre-1.6` 旧版资源；启动前快速补全缺失文件（资源只比较大小，约 150ms）；自动进入服务器（1.20+ 用 Quick Play，旧版用 `--server/--port`）；内存自动分配；日志实时推送；崩溃时分析常见原因并给出中文解决办法；启动后可隐藏 / 保持 / 关闭启动器。
- **界面**：无边框窗口 + 自定义标题栏；深色 / 浅色 / 跟随系统；主题色可自定义；动效只使用 transform / opacity，并尊重系统「减少动态效果」设置。

## 开发

### 环境要求

- Rust（stable，1.85 及以上）
- Node.js 20+ 与 pnpm 10
- 各平台的 Tauri 依赖，详见 <https://tauri.app/start/prerequisites/>
  - **Windows**：Microsoft C++ 生成工具（勾选「使用 C++ 的桌面开发」）与 WebView2（Windows 10/11 已自带）
  - **macOS**：Xcode Command Line Tools
  - **Linux（Debian/Ubuntu）**：
    ```sh
    sudo apt install libwebkit2gtk-4.1-dev build-essential libssl-dev libayatana-appindicator3-dev librsvg2-dev libxdo-dev libdbus-1-dev
    ```

### 运行

```sh
pnpm install
pnpm tauri dev
```

### 测试

```sh
# 核心库单元测试（规则判断、参数拼装、inheritsFrom 合并、离线 UUID、natives 解压等）
cargo test --workspace

# 使用真实 API 的集成测试：下载 1.20.1（约 730MB）与 Java 17，并启动游戏直到 LWJGL 初始化
# 默认游戏目录为 target/mc-test，可用 RC_TEST_GAME_DIR 指定
cargo test -p launcher-core --test vanilla -- --ignored --nocapture

# 加载器集成测试：Fabric / Quilt / Forge / NeoForge（1.20.1）以及 Forge 1.12.2、1.7.10，
# 安装后启动到加载器与 LWJGL 初始化。Linux 无显示器时需要 xvfb-run
xvfb-run -a cargo test -p launcher-core --test loaders -- --ignored --nocapture --test-threads 1

# 资源下载：Modrinth 搜索 + 解析 Sodium Extra 的前置（Sodium、Fabric API）并下载；
# CurseForge（MCIM 镜像）解析 Create: Steam 'n' Rails 的前置（Create）
cargo test -p launcher-core --test content -- --ignored --nocapture

# 整合包：安装 Fabulously Optimized（1.20.1）并启动到加载全部 Mod
xvfb-run -a cargo test -p launcher-core --test modpack -- --ignored --nocapture

# 前端类型检查
pnpm check
```

## 打包

```sh
pnpm tauri build
```

产物位于 `target/release/bundle/`。Windows 上默认生成 NSIS 安装包（`.exe`）和 MSI；也可以只生成其中一种：

```sh
pnpm tauri build --bundles nsis
pnpm tauri build --bundles msi
```

> MSI 需要 WiX Toolset v3，NSIS 需要 NSIS 3；Tauri CLI 会在首次打包时自动下载。MSI 只能在 Windows 上生成。

Release 配置已开启 `lto = true`、`opt-level = "s"`、`codegen-units = 1`、`panic = "abort"`、`strip = true`。

仓库的 GitHub Actions（`.github/workflows/build.yml`）会在推送到 main 时运行测试，并在 Windows / macOS / Linux 上打包，产物可在 Actions 页面下载。

## 申请 Azure 应用 Client ID（微软正版登录用）

微软登录默认使用浏览器授权（授权码 + PKCE，启动器在本机临时监听回调端口），也可以改用设备码流程（Device Code Flow）。需要你自己在 Azure 注册一个应用：

1. 打开 <https://portal.azure.com>，进入「Microsoft Entra ID」→「应用注册」→「新注册」。
2. 名称随意（例如 `Red Craft`）；「受支持的帐户类型」选择 **「任何组织目录中的帐户和个人 Microsoft 帐户」** 或 **「仅个人 Microsoft 帐户」**（必须包含个人账户，否则普通玩家无法登录）。
3. 「重定向 URI」留空即可（设备码流程不需要），点击「注册」。
4. 在应用的「身份验证」页面，把 **「允许公共客户端流」** 设为「是」并保存。
   然后点 **「添加平台」→「移动和桌面应用程序」**，在自定义重定向 URI 中填写 **`http://localhost`** 并保存。
   这是浏览器一键登录用的回调地址（端口由启动器随机选择，微软对 localhost 忽略端口）；不添加时只能使用验证码登录。
5. 在「概述」页面复制 **「应用程序(客户端) ID」**，这就是 Client ID。
6. **申请 Minecraft API 权限**：新注册的应用默认无法调用 Minecraft 的登录接口（`/authentication/login_with_xbox` 会返回 403 “Invalid app registration”）。需要填写微软的审核表单 <https://aka.ms/mce-reviewappid> 提交你的 Client ID，审核通过后才能正常登录，通常需要几天到几周。

### 配置 Client ID

代码里不写死 Client ID，有两种方式提供：

- **构建时注入**：设置环境变量 `REDCRAFT_MS_CLIENT_ID` 后再构建，例如
  ```sh
  REDCRAFT_MS_CLIENT_ID=xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx pnpm tauri dev
  ```
  仓库的 CI 已在 `.github/workflows/build.yml` 中设置了本项目的 Client ID。
- **运行时填写**：在启动器「设置 → 账号 → 微软登录 Client ID」中填写，会覆盖内置值。

可以用下面的联网测试检查 Azure 应用配置（只申请设备码，不需要真正登录）：

```sh
REDCRAFT_MS_CLIENT_ID=<你的 Client ID> cargo test -p launcher-core --test microsoft -- --ignored --nocapture
```

登录得到的令牌保存在系统密钥库（Windows 凭据管理器 / macOS 钥匙串 / Linux Secret Service），不会写入配置文件；Linux 上没有 Secret Service 时，退回到配置目录下 `secrets/` 中仅当前用户可读的文件。启动游戏时令牌过期会自动用 refresh token 刷新。

## 项目结构

```
├─ crates/launcher-core/   纯 Rust 核心库，不依赖 Tauri，可独立 cargo test
│  ├─ download/            并发下载器、HTTP 客户端、下载源（官方 / BMCLAPI）
│  ├─ version/             版本清单、版本 JSON、rules 判断、inheritsFrom 合并、libraries 展开
│  ├─ install/             原版安装与文件补全、资源索引
│  ├─ java/                Java 扫描、选择与 Mojang runtime 下载
│  ├─ auth/                账号与离线登录
│  ├─ launch/              参数拼装、natives 解压、启动进程与崩溃分析
│  ├─ instance.rs          实例扫描与加载器识别
│  └─ system.rs            物理内存与自动内存分配
├─ src-tauri/              Tauri 外壳：把 core 包装成 commands / events，管理设置与账号
└─ src/                    Svelte 5 前端
   ├─ components/          标题栏、侧栏、虚拟列表、弹窗、进度条等
   ├─ pages/               启动、实例、下载、Mod、账号、设置
   └─ lib/                 API 封装、全局状态、动效、格式化
```

启动器设置保存在系统配置目录下的 `RedCraft/`（Windows 为 `%APPDATA%\RedCraft`），便携模式下保存在 `.minecraft/redcraft/`；运行日志位于其中的 `logs/latest.log`。令牌等敏感信息不会写入这些文件。
