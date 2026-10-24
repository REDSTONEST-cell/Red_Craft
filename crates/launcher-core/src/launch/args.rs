//! 启动参数拼装：同时支持新版 `arguments` 与旧版 `minecraftArguments`。

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use crate::auth::LaunchAuth;
use crate::error::{CoreError, Result};
use crate::install::AssetIndex;
use crate::paths::GameDir;
use crate::version::{plan_libraries, rules_allow, Argument, Environment, ResolvedVersion};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerAddress {
    pub host: String,
    pub port: u16,
}

impl ServerAddress {
    /// 解析 `host[:port]`，支持 `[ipv6]:port`。
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim();
        if s.is_empty() {
            return None;
        }
        if let Some(rest) = s.strip_prefix('[') {
            let (host, tail) = rest.split_once(']')?;
            let port = match tail.strip_prefix(':') {
                Some(p) => p.parse().ok()?,
                None => 25565,
            };
            return Some(ServerAddress { host: host.into(), port });
        }
        match s.rsplit_once(':') {
            Some((host, port)) if !host.contains(':') => Some(ServerAddress {
                host: host.into(),
                port: port.parse().ok()?,
            }),
            _ => Some(ServerAddress {
                host: s.into(),
                port: 25565,
            }),
        }
    }
}

/// 内存回收器（GC）选择。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GcMode {
    /// 使用 G1（适合绝大多数情况）
    #[default]
    Auto,
    G1,
    /// 分代 ZGC：停顿更短，需要 Java 21+，不满足时退回 G1
    Zgc,
    /// 不添加 GC 参数，由 Java 决定
    None,
}

impl GcMode {
    pub fn args(self, java_major: u32) -> Vec<&'static str> {
        match self {
            GcMode::None => vec![],
            GcMode::Zgc if java_major >= 23 => vec!["-XX:+UseZGC"],
            GcMode::Zgc if java_major >= 21 => vec!["-XX:+UseZGC", "-XX:+ZGenerational"],
            _ => vec!["-XX:+UseG1GC", "-XX:-UseAdaptiveSizePolicy"],
        }
    }
}

#[derive(Debug, Clone)]
pub struct LaunchOptions {
    pub java: PathBuf,
    /// 游戏运行目录（版本隔离时为 `versions/<id>`）
    pub game_dir: PathBuf,
    pub auth: LaunchAuth,
    pub max_memory_mb: u32,
    pub min_memory_mb: u32,
    pub java_major: u32,
    pub gc: GcMode,
    /// 用户额外的 JVM 参数
    pub extra_jvm_args: Vec<String>,
    /// 用户额外的游戏参数
    pub extra_game_args: Vec<String>,
    pub resolution: Option<(u32, u32)>,
    pub fullscreen: bool,
    pub server: Option<ServerAddress>,
    pub launcher_name: String,
    pub launcher_version: String,
}

#[derive(Debug, Clone)]
pub struct LaunchCommand {
    pub java: PathBuf,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub natives_dir: PathBuf,
}

impl LaunchCommand {
    /// 用于日志显示的命令行，访问令牌会被隐藏。
    pub fn display(&self, token: &str) -> String {
        let mut s = self.java.display().to_string();
        for a in &self.args {
            let a = if !token.is_empty() && token.len() > 8 {
                a.replace(token, "********")
            } else {
                a.clone()
            };
            if a.contains(' ') {
                s.push_str(&format!(" \"{a}\""));
            } else {
                s.push(' ');
                s.push_str(&a);
            }
        }
        s
    }
}

/// 替换字符串中的 `${key}` 占位符，未知占位符保持原样。
pub fn substitute(input: &str, vars: &HashMap<&str, String>) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(start) = rest.find("${") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        match after.find('}') {
            Some(end) => {
                let key = &after[..end];
                match vars.get(key) {
                    Some(v) => out.push_str(v),
                    None => {
                        out.push_str("${");
                        out.push_str(key);
                        out.push('}');
                    }
                }
                rest = &after[end + 1..];
            }
            None => {
                out.push_str(&rest[start..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

fn has_feature(args: &[Argument], feature: &str) -> bool {
    args.iter().any(|a| match a {
        Argument::Conditional { rules, .. } => rules
            .iter()
            .any(|r| r.features.as_ref().is_some_and(|f| f.contains_key(feature))),
        Argument::Plain(_) => false,
    })
}

fn expand(args: &[Argument], env: &Environment, vars: &HashMap<&str, String>, out: &mut Vec<String>) {
    for a in args {
        match a {
            Argument::Plain(s) => out.push(substitute(s, vars)),
            Argument::Conditional { rules, value } => {
                if rules_allow(rules, env) {
                    out.extend(value.values().into_iter().map(|s| substitute(s, vars)));
                }
            }
        }
    }
}

/// 构建完整的启动命令。
pub fn build_command(
    game: &GameDir,
    resolved: &ResolvedVersion,
    env: &Environment,
    opts: &LaunchOptions,
) -> Result<LaunchCommand> {
    let json = &resolved.json;
    if json.main_class.is_empty() {
        return Err(CoreError::Launch(format!("版本 {} 没有指定主类（mainClass）", json.id)));
    }
    let id = json.id.as_str();
    let natives_dir = game.natives_dir(id);
    let sep = if env.os_name == "windows" { ";" } else { ":" };

    // classpath：库（去重）+ 客户端 jar
    let mut seen = HashSet::new();
    let mut classpath: Vec<String> = plan_libraries(&json.libraries, env)
        .into_iter()
        .filter(|l| l.on_classpath())
        .map(|l| game.library(&l.path).display().to_string())
        .filter(|p| seen.insert(p.clone()))
        .collect();
    classpath.push(game.version_jar(&resolved.jar_id).display().to_string());

    // 资源目录
    let asset_id = json
        .asset_index
        .as_ref()
        .map(|a| a.id.clone())
        .or_else(|| json.assets.clone())
        .unwrap_or_else(|| "legacy".into());
    let index = AssetIndex::read(&game.asset_index(&asset_id)).ok();
    let game_assets = match &index {
        Some(i) if i.map_to_resources => opts.game_dir.join("resources"),
        Some(i) if i.is_virtual => game.asset_virtual(&asset_id),
        _ => game.assets(),
    };

    let new_args = json.arguments.as_ref();
    let game_arg_list = new_args.map(|a| a.game.as_slice()).unwrap_or_default();
    let quick_play = opts.server.is_some() && has_feature(game_arg_list, "is_quick_play_multiplayer");

    let mut env = env.clone();
    env.features.insert("is_demo_user".into(), false);
    env.features
        .insert("has_custom_resolution".into(), opts.resolution.is_some());
    env.features.insert("has_quick_plays_support".into(), false);
    env.features.insert("is_quick_play_multiplayer".into(), quick_play);
    env.features.insert("is_quick_play_singleplayer".into(), false);
    env.features.insert("is_quick_play_realms".into(), false);

    let (width, height) = opts.resolution.unwrap_or((854, 480));
    let auth = &opts.auth;
    let mut vars: HashMap<&str, String> = HashMap::new();
    vars.insert("auth_player_name", auth.name.clone());
    vars.insert("version_name", id.to_string());
    vars.insert("game_directory", opts.game_dir.display().to_string());
    vars.insert("assets_root", game.assets().display().to_string());
    vars.insert("game_assets", game_assets.display().to_string());
    vars.insert("assets_index_name", asset_id.clone());
    vars.insert("auth_uuid", auth.uuid.clone());
    vars.insert("auth_access_token", auth.access_token.clone());
    vars.insert("auth_session", format!("token:{}:{}", auth.access_token, auth.uuid));
    vars.insert("auth_xuid", auth.xuid.clone());
    vars.insert("clientid", String::new());
    vars.insert("user_type", auth.user_type.clone());
    vars.insert("user_properties", "{}".into());
    vars.insert(
        "version_type",
        if json.kind.is_empty() { opts.launcher_name.clone() } else { json.kind.clone() },
    );
    vars.insert("natives_directory", natives_dir.display().to_string());
    vars.insert("launcher_name", opts.launcher_name.clone());
    vars.insert("launcher_version", opts.launcher_version.clone());
    vars.insert("classpath", classpath.join(sep));
    vars.insert("classpath_separator", sep.to_string());
    vars.insert("library_directory", game.libraries().display().to_string());
    vars.insert("primary_jar_name", format!("{}.jar", resolved.jar_id));
    vars.insert("resolution_width", width.to_string());
    vars.insert("resolution_height", height.to_string());
    if let Some(s) = &opts.server {
        vars.insert("quickPlayMultiplayer", format!("{}:{}", s.host, s.port));
    }

    let mut args = Vec::new();

    // 内存与用户 JVM 参数
    args.push(format!("-Xmx{}m", opts.max_memory_mb.max(512)));
    if opts.min_memory_mb > 0 {
        args.push(format!("-Xms{}m", opts.min_memory_mb.min(opts.max_memory_mb.max(512))));
    }
    let user_sets_gc = opts.extra_jvm_args.iter().any(|a| a.starts_with("-XX:+Use") && a.ends_with("GC"));
    if !user_sets_gc {
        args.extend(opts.gc.args(opts.java_major).into_iter().map(String::from));
    }
    args.push("-XX:-OmitStackTraceInFastThrow".into());
    args.push("-Dfml.ignoreInvalidMinecraftCertificates=true".into());
    args.push("-Dfml.ignorePatchDiscrepancies=true".into());
    args.push("-Dlog4j2.formatMsgNoLookups=true".into());
    // 让游戏输出 UTF-8，方便启动器正确显示中文日志
    args.push("-Dstdout.encoding=UTF-8".into());
    args.push("-Dstderr.encoding=UTF-8".into());
    args.push("-Dsun.stdout.encoding=UTF-8".into());
    args.push("-Dsun.stderr.encoding=UTF-8".into());
    if let Some(a) = &auth.authlib {
        args.push(format!("-javaagent:{}={}", a.jar.display(), a.server));
        args.push(format!("-Dauthlibinjector.yggdrasil.prefetched={}", a.prefetched));
    }
    args.extend(opts.extra_jvm_args.iter().cloned());

    // 日志配置（修复 Log4Shell 的官方配置）
    if let Some(log) = json.logging.as_ref().and_then(|l| l.client.as_ref()) {
        let path = game.log_configs().join(&log.file.id);
        if path.exists() {
            args.push(log.argument.replace("${path}", &path.display().to_string()));
        }
    }

    match new_args.filter(|a| !a.jvm.is_empty()) {
        Some(a) => {
            // Forge / NeoForge 用 `-DignoreList=...,${version_name}.jar` 排除原版 jar；
            // 实例名与原版 jar 名不同，这里换成实际的客户端 jar 名
            let jvm: Vec<Argument> = a
                .jvm
                .iter()
                .map(|arg| match arg {
                    Argument::Plain(s) if s.contains("${version_name}.jar") => {
                        Argument::Plain(s.replace("${version_name}.jar", "${primary_jar_name}"))
                    }
                    other => other.clone(),
                })
                .collect();
            expand(&jvm, &env, &vars, &mut args)
        }
        None => {
            // 旧版没有 JVM 参数，使用官方启动器的默认值
            if env.os_name == "osx" {
                args.push("-XstartOnFirstThread".into());
            }
            if env.os_name == "windows" {
                args.push("-XX:HeapDumpPath=MojangTricksIntelDriversForPerformance_javaw.exe_minecraft.exe.heapdump".into());
            }
            if env.arch == "x86" {
                args.push("-Xss1M".into());
            }
            args.push(format!("-Djava.library.path={}", natives_dir.display()));
            args.push(format!("-Dminecraft.launcher.brand={}", opts.launcher_name));
            args.push(format!("-Dminecraft.launcher.version={}", opts.launcher_version));
            args.push("-cp".into());
            args.push(vars["classpath"].clone());
        }
    }

    args.push(json.main_class.clone());

    let mut game_args = Vec::new();
    if let Some(a) = new_args.filter(|a| !a.game.is_empty()) {
        expand(&a.game, &env, &vars, &mut game_args);
    }
    if let Some(legacy) = &json.minecraft_arguments {
        // Forge 等合并版本可能同时带有两种写法，旧写法只在没有新写法时使用
        if game_args.is_empty() {
            game_args.extend(legacy.split_whitespace().map(|s| substitute(s, &vars)));
        }
    }
    let legacy_format = new_args.is_none_or(|a| a.game.is_empty());
    if legacy_format {
        if let Some((w, h)) = opts.resolution {
            game_args.extend(["--width".into(), w.to_string(), "--height".into(), h.to_string()]);
        }
    }
    if let Some(s) = &opts.server {
        if !quick_play {
            game_args.extend(["--server".into(), s.host.clone(), "--port".into(), s.port.to_string()]);
        }
    }
    if opts.fullscreen {
        game_args.push("--fullscreen".into());
    }
    game_args.extend(opts.extra_game_args.iter().cloned());
    args.extend(game_args);

    Ok(LaunchCommand {
        java: opts.java.clone(),
        args,
        cwd: opts.game_dir.clone(),
        natives_dir,
    })
}

/// 按空白拆分用户填写的参数，支持双引号包裹含空格的参数。
pub fn split_args(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_quotes = false;
    let mut has = false;
    for c in s.chars() {
        match c {
            '"' => {
                in_quotes = !in_quotes;
                has = true;
            }
            c if c.is_whitespace() && !in_quotes => {
                if has {
                    out.push(std::mem::take(&mut cur));
                    has = false;
                }
            }
            c => {
                cur.push(c);
                has = true;
            }
        }
    }
    if has {
        out.push(cur);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::version::{resolve_with, VersionJson};

    fn env() -> Environment {
        Environment {
            os_name: "windows".into(),
            os_version: "10.0".into(),
            arch: "x86_64".into(),
            features: HashMap::new(),
        }
    }

    fn opts(game_dir: &std::path::Path) -> LaunchOptions {
        LaunchOptions {
            java: "java".into(),
            game_dir: game_dir.to_path_buf(),
            auth: LaunchAuth::offline("Steve"),
            max_memory_mb: 4096,
            min_memory_mb: 1024,
            java_major: 17,
            gc: GcMode::Auto,
            extra_jvm_args: vec!["-Dfoo=bar".into()],
            extra_game_args: vec![],
            resolution: None,
            fullscreen: false,
            server: None,
            launcher_name: "RedCraft".into(),
            launcher_version: "0.1.0".into(),
        }
    }

    fn resolved(json: &str) -> ResolvedVersion {
        let v: VersionJson = serde_json::from_str(json).unwrap();
        resolve_with(&v.id.clone(), |_| Ok(v.clone())).unwrap()
    }

    const MODERN: &str = r#"{
        "id": "1.20.1", "type": "release", "mainClass": "net.minecraft.client.main.Main",
        "assetIndex": {"id": "5", "url": "https://x/5.json"},
        "arguments": {
            "game": [
                "--username", "${auth_player_name}", "--version", "${version_name}",
                "--gameDir", "${game_directory}", "--assetsDir", "${assets_root}",
                "--assetIndex", "${assets_index_name}", "--uuid", "${auth_uuid}",
                "--accessToken", "${auth_access_token}", "--userType", "${user_type}",
                "--versionType", "${version_type}",
                {"rules": [{"action": "allow", "features": {"is_demo_user": true}}], "value": "--demo"},
                {"rules": [{"action": "allow", "features": {"has_custom_resolution": true}}],
                 "value": ["--width", "${resolution_width}", "--height", "${resolution_height}"]},
                {"rules": [{"action": "allow", "features": {"is_quick_play_multiplayer": true}}],
                 "value": ["--quickPlayMultiplayer", "${quickPlayMultiplayer}"]}
            ],
            "jvm": [
                {"rules": [{"action": "allow", "os": {"name": "osx"}}], "value": ["-XstartOnFirstThread"]},
                {"rules": [{"action": "allow", "os": {"name": "windows"}}],
                 "value": "-XX:HeapDumpPath=MojangTricksIntelDriversForPerformance_javaw.exe_minecraft.exe.heapdump"},
                "-Djava.library.path=${natives_directory}",
                "-Dminecraft.launcher.brand=${launcher_name}",
                "-Dminecraft.launcher.version=${launcher_version}",
                "-cp", "${classpath}"
            ]
        },
        "libraries": [
            {"name": "a:b:1", "downloads": {"artifact": {"path": "a/b/1/b-1.jar", "url": "u"}}},
            {"name": "a:b:1:natives-windows", "downloads": {"artifact": {"path": "a/b/1/b-1-natives-windows.jar", "url": "u"}},
             "rules": [{"action": "allow", "os": {"name": "windows"}}]},
            {"name": "c:d:1", "downloads": {"artifact": {"path": "c/d/1/d-1.jar", "url": "u"}},
             "rules": [{"action": "allow", "os": {"name": "osx"}}]}
        ]
    }"#;

    fn value_after<'a>(args: &'a [String], key: &str) -> &'a str {
        let i = args.iter().position(|a| a == key).unwrap_or_else(|| panic!("缺少 {key}"));
        &args[i + 1]
    }

    #[test]
    fn modern_arguments() {
        let dir = tempfile::tempdir().unwrap();
        let game = GameDir::new(dir.path());
        let gd = game.version_dir("1.20.1");
        let cmd = build_command(&game, &resolved(MODERN), &env(), &opts(&gd)).unwrap();
        let a = &cmd.args;

        assert_eq!(a[0], "-Xmx4096m");
        assert_eq!(a[1], "-Xms1024m");
        assert!(a.contains(&"-Dfoo=bar".to_string()));
        assert!(!a.contains(&"-XstartOnFirstThread".to_string()));
        assert!(a.iter().any(|x| x.starts_with("-XX:HeapDumpPath")));
        assert!(a.contains(&"-Dminecraft.launcher.brand=RedCraft".to_string()));
        assert!(a.contains(&format!("-Djava.library.path={}", game.natives_dir("1.20.1").display())));

        let cp = value_after(a, "-cp");
        let parts: Vec<_> = cp.split(';').collect();
        assert_eq!(parts.len(), 3, "{cp}");
        assert!(parts[0].ends_with("b-1.jar"));
        assert!(parts[1].ends_with("b-1-natives-windows.jar"));
        assert!(parts[2].ends_with("1.20.1.jar"));

        let main = a.iter().position(|x| x == "net.minecraft.client.main.Main").unwrap();
        assert_eq!(a[main - 1], cp);
        assert_eq!(value_after(a, "--username"), "Steve");
        assert_eq!(value_after(a, "--version"), "1.20.1");
        assert_eq!(value_after(a, "--gameDir"), gd.display().to_string());
        assert_eq!(value_after(a, "--assetIndex"), "5");
        assert_eq!(value_after(a, "--uuid"), "5627dd98e6be3c21b8a8e92344183641");
        assert_eq!(value_after(a, "--userType"), "legacy");
        assert_eq!(value_after(a, "--versionType"), "release");
        assert!(!a.contains(&"--demo".to_string()));
        assert!(!a.contains(&"--width".to_string()));
        assert!(!a.iter().any(|x| x.contains("${")), "{a:?}");
    }

    #[test]
    fn modern_resolution_and_quick_play() {
        let dir = tempfile::tempdir().unwrap();
        let game = GameDir::new(dir.path());
        let mut o = opts(dir.path());
        o.resolution = Some((1280, 720));
        o.server = ServerAddress::parse("mc.example.com:25566");
        let cmd = build_command(&game, &resolved(MODERN), &env(), &o).unwrap();
        assert_eq!(value_after(&cmd.args, "--width"), "1280");
        assert_eq!(value_after(&cmd.args, "--height"), "720");
        assert_eq!(value_after(&cmd.args, "--quickPlayMultiplayer"), "mc.example.com:25566");
        assert!(!cmd.args.contains(&"--server".to_string()));
    }

    const LEGACY: &str = r#"{
        "id": "1.7.10", "type": "release", "mainClass": "net.minecraft.client.main.Main",
        "assets": "1.7.10",
        "minecraftArguments": "--username ${auth_player_name} --session ${auth_session} --version ${version_name} --gameDir ${game_directory} --assetsDir ${game_assets} --assetIndex ${assets_index_name} --uuid ${auth_uuid} --accessToken ${auth_access_token} --userProperties ${user_properties} --userType ${user_type}",
        "libraries": [
            {"name": "org.lwjgl.lwjgl:lwjgl-platform:2.9.1",
             "natives": {"windows": "natives-windows", "linux": "natives-linux"},
             "downloads": {"classifiers": {"natives-windows": {"path": "n.jar", "url": "u"}}}},
            {"name": "org.lwjgl.lwjgl:lwjgl:2.9.1"}
        ]
    }"#;

    #[test]
    fn legacy_arguments() {
        let dir = tempfile::tempdir().unwrap();
        let game = GameDir::new(dir.path());
        let mut o = opts(dir.path());
        o.server = ServerAddress::parse("127.0.0.1");
        o.resolution = Some((1000, 600));
        let cmd = build_command(&game, &resolved(LEGACY), &env(), &o).unwrap();
        let a = &cmd.args;
        assert!(a.iter().any(|x| x.starts_with("-Djava.library.path=")));
        assert!(a.contains(&"-Dminecraft.launcher.brand=RedCraft".to_string()));
        let cp = value_after(a, "-cp");
        // natives 不应出现在 classpath
        assert!(!cp.contains("n.jar"));
        assert!(cp.contains("lwjgl-2.9.1.jar"));
        assert_eq!(value_after(a, "--userProperties"), "{}");
        assert!(value_after(a, "--session").starts_with("token:"));
        // 没有资源索引文件时退回 assets 根目录
        assert_eq!(value_after(a, "--assetsDir"), game.assets().display().to_string());
        assert_eq!(value_after(a, "--server"), "127.0.0.1");
        assert_eq!(value_after(a, "--port"), "25565");
        assert_eq!(value_after(a, "--width"), "1000");
        assert!(!a.iter().any(|x| x.contains("${")), "{a:?}");
    }

    #[test]
    fn legacy_virtual_assets() {
        let dir = tempfile::tempdir().unwrap();
        let game = GameDir::new(dir.path());
        std::fs::create_dir_all(game.assets().join("indexes")).unwrap();
        std::fs::write(game.asset_index("legacy"), r#"{"virtual": true, "objects": {}}"#).unwrap();
        std::fs::write(game.asset_index("pre-1.6"), r#"{"map_to_resources": true, "objects": {}}"#).unwrap();
        let mk = |assets: &str| {
            resolved(&LEGACY.replace(r#""assets": "1.7.10""#, &format!(r#""assets": "{assets}""#)))
        };
        let cmd = build_command(&game, &mk("legacy"), &env(), &opts(dir.path())).unwrap();
        assert_eq!(value_after(&cmd.args, "--assetsDir"), game.asset_virtual("legacy").display().to_string());
        let cmd = build_command(&game, &mk("pre-1.6"), &env(), &opts(dir.path())).unwrap();
        assert_eq!(value_after(&cmd.args, "--assetsDir"), dir.path().join("resources").display().to_string());
    }

    #[test]
    fn forge_ignore_list_uses_client_jar() {
        let dir = tempfile::tempdir().unwrap();
        let game = GameDir::new(dir.path());
        let child: VersionJson = serde_json::from_str(
            r#"{"id":"my-forge","inheritsFrom":"1.20.1","mainClass":"cpw.mods.bootstraplauncher.BootstrapLauncher",
                "arguments":{"jvm":["-DignoreList=fmlcore,forge-,${version_name}.jar"],"game":[]}}"#,
        )
        .unwrap();
        let parent: VersionJson = serde_json::from_str(MODERN).unwrap();
        let r = resolve_with("my-forge", |id| Ok(if id == "my-forge" { child.clone() } else { parent.clone() })).unwrap();
        let cmd = build_command(&game, &r, &env(), &opts(dir.path())).unwrap();
        assert!(cmd.args.contains(&"-DignoreList=fmlcore,forge-,1.20.1.jar".to_string()), "{:?}", cmd.args);
        assert_eq!(value_after(&cmd.args, "--version"), "my-forge");
    }

    #[test]
    fn gc_modes() {
        assert_eq!(GcMode::Auto.args(17), vec!["-XX:+UseG1GC", "-XX:-UseAdaptiveSizePolicy"]);
        assert_eq!(GcMode::Zgc.args(17), vec!["-XX:+UseG1GC", "-XX:-UseAdaptiveSizePolicy"]);
        assert_eq!(GcMode::Zgc.args(21), vec!["-XX:+UseZGC", "-XX:+ZGenerational"]);
        assert_eq!(GcMode::Zgc.args(25), vec!["-XX:+UseZGC"]);
        assert!(GcMode::None.args(21).is_empty());
    }

    #[test]
    fn placeholder_substitution() {
        let mut vars = HashMap::new();
        vars.insert("a", "1".to_string());
        assert_eq!(substitute("x${a}y${a}", &vars), "x1y1");
        assert_eq!(substitute("${unknown}", &vars), "${unknown}");
        assert_eq!(substitute("tail ${a", &vars), "tail ${a");
        assert_eq!(substitute("中文${a}", &vars), "中文1");
    }

    #[test]
    fn server_parse() {
        assert_eq!(ServerAddress::parse("a.com"), Some(ServerAddress { host: "a.com".into(), port: 25565 }));
        assert_eq!(ServerAddress::parse("a.com:1"), Some(ServerAddress { host: "a.com".into(), port: 1 }));
        assert_eq!(ServerAddress::parse("[::1]:2"), Some(ServerAddress { host: "::1".into(), port: 2 }));
        assert_eq!(ServerAddress::parse(""), None);
        assert_eq!(ServerAddress::parse("a.com:x"), None);
    }

    #[test]
    fn split_user_args() {
        assert_eq!(split_args(r#" -Da=1  "-Db=x y" -Dc"#), vec!["-Da=1", "-Db=x y", "-Dc"]);
        assert!(split_args("   ").is_empty());
    }

    #[test]
    fn token_hidden_in_display() {
        let cmd = LaunchCommand {
            java: "java".into(),
            args: vec!["--accessToken".into(), "secret-token-123".into()],
            cwd: ".".into(),
            natives_dir: ".".into(),
        };
        assert!(!cmd.display("secret-token-123").contains("secret"));
    }
}
