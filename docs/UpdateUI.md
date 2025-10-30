# UpdateUI 更新说明

## 本次改动摘要
- **配置自动生成**：教师端与学生端在启动时若缺少配置文件，会自动写出默认模板，避免首次部署时的手工拷贝。
- **可选 UI 控制面板**：新增 `ui` Feature，教师端可在 Windows 上使用原生窗口查看学生列表、切换广播模式、分发文件、控制音频。
- **指令通道统一**：教师端内部增加命令分发机制，CLI 与 UI 使用同一套逻辑，避免重复实现。

## 使用方式
1. 默认仍以命令行方式运行：
   ```powershell
   cargo run --release --bin teacher -- --config .\configs\teacher_config.toml
   ```
2. 启动 UI 面板时需启用 Feature：
   ```powershell
   cargo run --release --features ui --bin teacher -- --config .\configs\teacher_config.toml
   ```
3. 运行前请确认自动生成的 `configs/teacher_config.toml` 与 `configs/student_config.json` 已根据实际环境调整。

## 后续计划
- 为 UI 增加状态刷新动画、文件传输进度等体验优化。
- 在网络层逐步引入 TLS 加密与带宽控制，提升在复杂网络环境下的稳定性。
- 扩展配置项，让自动生成的模板更贴近教学现场的常见需求。
