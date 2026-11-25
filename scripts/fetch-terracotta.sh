#!/usr/bin/env bash
# 下载并校验 Terracotta | 陶瓦联机 的官方发行包，放到 src-tauri/terracotta/<目标平台>.tar.gz，
# 构建时由 src-tauri/build.rs 嵌入启动器（未经修改的二进制，见 Terracotta 许可证的例外条款）。
#
# 用法：scripts/fetch-terracotta.sh [目标三元组]   （缺省为本机 rustc 的 host）
set -euo pipefail

VERSION="0.4.2"
target="${1:-$(rustc -vV | sed -n 's/^host: //p')}"

case "$target" in
  x86_64-pc-windows-msvc)     asset="windows-x86_64"; sha="07ebe139e3ca5f74576e58b1a96efe59abdfbe148d3f1a49bfdca8b6f70745f0" ;;
  aarch64-pc-windows-msvc)    asset="windows-arm64";  sha="acfab0a87a02dedc6dab7c05303186c8907f56f815548b693fb3324358da7d14" ;;
  x86_64-unknown-linux-gnu)   asset="linux-x86_64";   sha="675c4fd6c74d49ed8165151ba2be5b6582e0af20fb6d912074543c2484b1e10a" ;;
  aarch64-unknown-linux-gnu)  asset="linux-arm64";    sha="845285ff264ac5fbc16db1a1605ad190e7fa64196516068cc309de5a1d2bf66d" ;;
  x86_64-apple-darwin)        asset="macos-x86_64";   sha="16306157d89423ce79fa901cdb75a6386ec1a9b1bd43a5d47c2c47cf01a16b86" ;;
  aarch64-apple-darwin)       asset="macos-arm64";    sha="13de7f9ce8733971b23493fabbe7e16d480f1e0d16a6265b4861f5a01bbecb60" ;;
  *) echo "陶瓦联机没有 $target 的发行包，跳过（启动器的联机功能在该平台不可用）"; exit 0 ;;
esac

# 计算 SHA256：GNU sha256sum（Linux、Windows 的 Git Bash）；macOS 的 sha256sum 是 BSD 版、
# 不支持 GNU 的参数，改用 shasum。只取哈希值比较，不依赖各平台 -c 参数的差异。
digest() {
  if sha256sum --version >/dev/null 2>&1; then sha256sum "$1" | awk '{print $1}'
  else shasum -a 256 "$1" | awk '{print $1}'; fi
}
check() {
  [ "$(digest "$2")" = "$1" ]
}

dir="$(cd "$(dirname "$0")/.." && pwd)/src-tauri/terracotta"
mkdir -p "$dir"
out="$dir/$target.tar.gz"
url="https://github.com/burningtnt/Terracotta/releases/download/v$VERSION/terracotta-$VERSION-$asset-pkg.tar.gz"

if [ -f "$out" ] && check "$sha" "$out" 2>/dev/null; then
  echo "已存在且校验通过：$out"
  exit 0
fi
echo "下载 $url"
curl -fsSL --retry 4 -o "$out.part" "$url"
if ! check "$sha" "$out.part"; then
  echo "SHA256 校验失败：$url" >&2
  rm -f "$out.part"
  exit 1
fi
mv "$out.part" "$out"
echo "已保存：$out"
