# FJCPC Classroom Broadcast

基于 Rust 的 Windows 课堂广播系统，为教师端与学生端提供屏幕/音频直播、文件分发及交互控制能力。

## 功能亮点

- **实时广播**：教师端可将屏幕与音频同步发送给所有学生，支持窗口/全屏模式切换。
- **学生聚焦**：支持指定学生并广播其屏幕，方便课堂展示。
- **文件往返**：教师端集中下发资料，学生端可回传作业，系统按学生 ID 自动分组存放。
- **配置自修复**：启动时若发现缺失的 `configs/teacher_config.toml` 或 `configs/student_config.json`，程序会自动写出默认模板，减少部署成本。
- **可选 UI 面板**：在启用 `ui` Feature 时提供本地 Windows 控制台，直观管理学生列表与广播状态。

## 快速开始

### 环境准备
1. 安装 Rust 稳定工具链。
2. 克隆仓库并进入项目目录：
   ```powershell
   git clone <repo>
   cd fjcpc-classroom-broadcast
   ```
3. 首次运行会自动生成默认配置文件，随后请根据现场环境修订 `configs/teacher_config.toml` 与 `configs/student_config.json` 中的监听地址、端口、学生信息等内容。

### 教师端（命令行）
```powershell
cargo run --release --bin teacher -- --config .\configs\teacher_config.toml
```
常用控制命令包含：`help`、`students`、`start [window]`、`stop`、`spotlight <student_id>`、`send <path> [open]`、`audio <on|off|force|allow>`、`quit`。

### 教师端 UI 控制面板（可选）
启用 `ui` Feature 后，可在 Windows 上调出原生窗口界面（包含学生列表、广播状态、文件分发按钮等）：
```powershell
cargo run --release --features ui --bin teacher -- --config .\configs\teacher_config.toml
```
UI 与 CLI 共用底层逻辑，任一端的操作都会同步到另一端。

### 学生端
```powershell
cargo run --release --bin student -- --config .\configs\student_config.json
```
学生端默认将教师分发的文件保存到配置中的 `download_path`，上传文件则会按学生 ID 分类存储到教师端的上传目录。

## 项目结构
```
configs/   # 配置模板与运行时配置
shared/    # 通用协议、类型与工具
teacher/   # 教师端 crate
student/   # 学生端 crate
docs/      # 设计/更新说明文档
```

## 使用与部署提示
1. 推荐在 Release 构建下运行，可获得更好的编码与网络性能。
2. 音频广播依赖 CPAL，请确保系统存在可用的输入/输出设备并开放访问权限。
3. 目前数据传输基于 TCP，若需跨公网或对安全性有更高要求，请在外层配合 VPN/TLS 等方案。
4. UI 仍为基础版本，后续计划补充文件进度提示、状态刷新动画等增强体验。

欢迎提交 Issue 或 PR 共同完善项目。
