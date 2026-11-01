# RCL（Red Craft Launcher）

轻量、快速的 Minecraft Java 版启动器，基于 Tauri 2 + Rust + Svelte 5。

当前正式版：**1.1.5** · [下载](https://github.com/REDSTONEST-cell/Red_Craft/releases)

## 下载

| 系统 | 文件 | 说明 |
| --- | --- | --- |
| Windows | `RCL_<版本>_x64win.exe` | 单文件，免安装，数据保存在 exe 所在文件夹；默认以管理员身份运行 |
| macOS | `RCL_<版本>_aarch64.dmg` | |
| Linux | `.deb` / `.AppImage` | |

手机版见 [RCL_Android](https://github.com/REDSTONEST-cell/RCL_Android)。

## 功能

**游戏**
- 安装任意 Minecraft 版本，可同时安装 Fabric / Quilt / Forge / NeoForge、OptiFine、Iris / Oculus
- 自动选择或下载合适的 Java；内存自动分配；版本隔离
- 启动前快速补全文件；崩溃时分析原因并给出中文解决办法
- 每个版本第一次启动时自动设为简体中文
- 从官方启动器、HMCL、PCL、Prism、MultiMC 导入已有的版本与账号

**账号**
- 离线、微软正版、外置登录（LittleSkin 等 authlib-injector 服务器）
- 令牌保存在系统密钥库，过期自动刷新

**资源**
- 同时搜索 Modrinth 与 CurseForge，支持中文名搜索（如「万用皮肤补丁」）
- 自动安装前置；整合包安装、导入与导出
- 资源管理：统一管理 Mod、材质包、光影包与存档，启用 / 停用、一键切换光影、检查更新

**下载**
- 大文件多线程分块下载，官方源与镜像（BMCLAPI、MCIM）同时使用，慢了自动换线路
- 文件下载源、版本列表源、社区资源源可分别设置；支持限速

**界面**
- 利落 / 圆润 / 毛玻璃三种形状，深色 / 浅色，自定义主题色
- 自定义背景：图片、视频、Wallpaper Engine 视频壁纸
- 启动前内存优化（Windows）；启动器内检查更新，下载后校验 SHA256

## 开发

需要 Rust（1.85+）、Node.js 20+ 与 pnpm 10，以及 [Tauri 的系统依赖](https://tauri.app/start/prerequisites/)。

```sh
pnpm install
pnpm tauri dev        # 运行
pnpm check            # 前端类型检查
cargo test --workspace
```

联网集成测试默认忽略，需要时加 `-- --ignored --nocapture` 运行，例如：

```sh
cargo test -p launcher-core --test vanilla -- --ignored --nocapture      # 安装并启动 1.20.1
xvfb-run -a cargo test -p launcher-core --test loaders -- --ignored --nocapture --test-threads 1
```

打包：`pnpm tauri build`。Windows 单文件版：`pnpm tauri build --no-bundle --features portable`。
推送到 main 时 GitHub Actions 会运行测试并在三个平台上打包。

### 微软登录的 Client ID

微软登录需要一个 Azure 应用的 Client ID：

1. 在 [Azure 门户](https://portal.azure.com) →「Microsoft Entra ID」→「应用注册」新建应用，帐户类型需包含**个人 Microsoft 帐户**。
2. 在「身份验证」中开启「允许公共客户端流」，并添加「移动和桌面应用程序」平台，重定向 URI 填 `http://localhost`。
3. 通过 <https://aka.ms/mce-reviewappid> 申请 Minecraft API 权限（审核通过前登录会返回 403）。
4. 构建时设置环境变量 `REDCRAFT_MS_CLIENT_ID`，或在「设置 → 账号」中填写。

## 项目结构

```
crates/launcher-core/   Rust 核心：下载、版本、安装、Java、账号、启动、资源（不依赖 Tauri）
src-tauri/              Tauri 外壳：commands / events、设置与账号
src/                    Svelte 前端（src/mobile 为手机版界面）
```

设置保存在 `%APPDATA%\RedCraft`（单文件版在 `.minecraft/redcraft/`），日志在其中的 `logs/latest.log`。

## 致谢

- 下载镜像：[BMCLAPI](https://bmclapidoc.bangbang93.com/)、[MCIM](https://mod.mcimirror.top)（含资源简介中文翻译）
- 中文搜索与聚合搜索的思路参考了 PCL
- 自定义背景功能由 @Windshakes 贡献
