# 开发工具

本目录存放仍可重复运行、独立于生产客户端的探针。构建缓存和生成语料放在工具自己的 `target/`，不提交到 Git。

- [logstore-probe](logstore-probe/README.md)：生成日志语料，测量 tiderm-textdoc 存储、索引和压缩。历史 G4-A 数据保留在工具 README。

仓库级门禁、发布与环境脚本在 [scripts/](../scripts/)；Qt 专用脚本在 [native/scripts/](../native/scripts/)。
