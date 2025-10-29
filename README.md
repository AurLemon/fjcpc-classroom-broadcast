# FJCPC Classroom Broadcast

Rust 实现的 Windows 课堂广播与文件分发系统，提供教师端与学生端两个客户端，支持屏幕/音频同步广播、文件分发与回收、学生屏幕展示等核心教学场景。

## 功能概览

- 教师端屏幕捕获并广播给多台学生机。
- 教师端可选择学生机屏幕进行全班展示。
- 文件分发（教师 -> 学生）与文件上传（学生 -> 教师）。
- 教师端音频推流，学生端音频播放，可选强制取消静音。
- 学生端命令行支持上传、静音切换等基础操作。

## 构建与运行

### 准备

1. 安装 Rust 工具链。
2. 克隆本仓库并进入根目录：

       git clone <repo>
       cd fjcpc-classroom-broadcast

3. 根据实际环境调整 configs/teacher_config.toml 与 configs/student_config.json。

### 教师端

       cargo run --bin teacher -- --config .\configs\teacher_config.toml

常用命令：

- `help`：查看命令列表
- `students`：查看在线学生
- `start [window]`：开启教师屏幕广播，可选 `window` 切换窗口模式
- `stop`：停止当前广播
- `spotlight <student_id>`：请求学生共享屏幕
- `send <路径> [open]`：分发文件，可选 `open` 请求自动打开
- `audio <on|off|force|allow>`：管理音频广播
- `quit`：退出教师端

### 学生端

       cargo run --bin student -- --config .\configs\student_config.json

学生端命令：

- `upload <路径>`：上传文件到教师端
- `mute` / `unmute`：切换音频播放
- `help`：查看命令列表
- `quit`：退出学生端

学生端默认将教师推送的文件保存至配置项 `download_path` 指定目录；上传的文件在教师端按学生 ID 划分保存。

## 目录结构

```
configs/                # 样例配置
shared/                 # 公共协议、配置与工具
teacher/                # 教师端二进制 crate
student/                # 学生端二进制 crate
docs/                   # 需求文档等附加说明
```

## 已知限制

- 学生端窗口模式暂未实现系统级全屏切换（受 minifb 0.24 限制）。
- 当前实现采用 TCP 传输，尚未引入差分编码、带宽优化等高级特性。
- 命令行交互简单，未来可扩展图形界面与托盘控制。

欢迎根据实际教学需求继续扩展完善。
