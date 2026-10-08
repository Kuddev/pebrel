# 主题 ZIP / Theme ZIP

一个 ZIP 包含主题 JSON、作者信息和图片资源，导入后可以离线使用。

```text
example.pebrel-theme.zip
  manifest.json
  theme.pebrel-theme.json
  assets/
    background.png
    preview.jpg
```

## 边界 / Boundary

包核心负责校验、带资源导出和本地安装。CLI 与编辑器入口在各自的适配层接入，
共同调用本模块，不重复实现大小、路径或完整性规则。

安装只添加到本地主题库，不自动应用运行设置。本阶段支持静态背景及可选预览图；
动态媒体不被本模块激活。删除主题 JSON 目前保留资源目录，自动清理尚未接入。

The package core validates archives, streams portable exports, and installs
managed resources. CLI and editor adapters share this boundary. Installation
adds a library theme without applying runtime preferences. Resource directories
currently remain after deleting a library JSON; automatic cleanup is separate.

## 体积 / Size

| 项目 / Item | 上限 / Maximum |
| --- | --- |
| ZIP 文件 / Archive | 48 MiB |
| 解压总量 / Unpacked total | 64 MiB |
| 单视频及视频合计 / One video and video total | 32 MiB |
| 清单 / Manifest | 64 KiB |
| 文件数 / Entries | 64 |

实际字节数、路径和 SHA-256 都会检查。打包或导入不会执行包内代码。
包模块的 Rust 常量是安装限制的唯一实现。

图片路径在包内使用相对路径，导入时转换为安装目录内的本地路径。
不接受路径穿越、盘符、设备文件、符号链接、重复文件名、加密或分卷 ZIP。
格式版本 1 使用普通 ZIP；Zip64、ZIP 文件注释与目录占位条目不属于本阶段合同。

The installer enforces actual stream sizes and SHA-256, validates portable
relative paths, and rejects undeclared entries. Video/animation/shader resource
kinds reserve metadata for later capabilities; this build does not activate them.
Callers perform cold operations without resident scans or media decoding.
