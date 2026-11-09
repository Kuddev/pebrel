# Sharing theme packages

Export the native JSON from the editor, then include its background and an optional preview:

```sh
pebrel theme pack example.pebrel-theme.json --output example.pebrel-theme.zip --author "Your name" --license CC-BY-4.0 --version 1.0.0 --preview preview.png
pebrel theme check example.pebrel-theme.zip
pebrel theme import example.pebrel-theme.zip
pebrel theme limits
pebrel theme catalog
pebrel theme install xvshifu/rice-paper
```

`catalog` fetches the [community catalog](https://github.com/pebrel-community/themes)
and marks entries already in your library. `install` downloads one entry, checks
it against the size and SHA-256 recorded in the catalog, then imports it like a
local ZIP. Both commands contact the network only when you run them.

`--github` and `--preview` are optional. Relative background paths resolve from
the JSON's directory. Import adds a library document without applying settings.
This build installs static image resources; it rejects animated media, video,
and shaders before extraction. A successful check is integrity/format validation,
not a playback acceptance result.

`theme catalog` 列出社区目录并标出已安装的条目；`theme install <ID>` 下载一项，
按目录登记的大小和 SHA-256 校验后照常导入。只有执行这两个命令时才会联网。

先从编辑器导出 Pebrel JSON，再使用 `theme pack` 一起分享背景和可选预览图。
导入只添加到主题库，不自动应用。当前不安装视频、动画或着色器；包校验成功
不表示播放已经通过验收。体积规则见 [主题包格式](README.md)。
