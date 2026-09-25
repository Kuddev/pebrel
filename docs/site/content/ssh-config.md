## 给连接取一个别名

SSH config 可以把地址、用户名、端口和密钥路径放在一个有名称的条目里。用户配置通常位于 `~/.ssh/config`；Windows 对应用户目录下的 `.ssh\config`。

例如，为开发服务器添加：

```sshconfig
Host development
    HostName example.com
    User dev
    Port 22
    IdentityFile ~/.ssh/id_ed25519
```

把地址、账户和私钥路径换成自己的信息。`Host development`定义别名，`HostName`才是真正连接的服务器地址。

## 在 Pebrel 中使用

1. 保存 SSH config 文件。
2. 打开 **设置 → SSH**。
3. 使用重新读取入口刷新已发现的主机。
4. 查找 `development`并启动连接。
5. 首次连接时核对指纹，再处理认证提示。

如果已经另外保存过同一服务器的主机条目，可以给两者设置容易区分的名称，避免误用不同账户。

## 通过跳板机连接

配置中可以使用一个独立别名描述跳板，例如：

```sshconfig
Host bastion
    HostName bastion.example.com
    User gateway

Host internal-dev
    HostName 10.0.0.20
    User dev
    ProxyJump bastion
```

然后在主机的 **高级**设置中，让跳板策略 **跟随 SSH config**。也可以改为 **指定主机**，为这一条连接单独设置跳板。界面中的连接规则预览可帮助确认实际路线。

涉及多个跳板、复杂匹配或其他 OpenSSH 选项时，先查看 Pebrel 的连接预览并测试；并非所有外部 SSH 配置选项都具有相同支持范围。代理与跳板的具体操作见[连接路由](ssh-routing.md)。

## 修改后没有生效

确认修改的是当前用户的配置文件，保存后重新读取，再新建连接。已经建立的 SSH 会话不会中途切换到新地址或新密钥。

在 Pebrel 中保存的主机覆盖项也可能影响连接。检查 **高级**页是否设置为跟随配置，还是显式指定了代理或跳板。

## 隐藏或移除别名

只是不想在 Pebrel 列表中看到某个别名时，可以隐藏它。想从 SSH 配置本身移除连接，则编辑原文件并删除相应段落，再刷新列表。修改原文件前保留备份，尤其是在其他 SSH 客户端也使用它的情况下。
