# Waypaper-rs

一个基于 Rust 和 Wayland 的动态壁纸应用程序，支持视频壁纸播放。

> **注意**：本项目的大部分代码由 AI 辅助生成。

## 功能特性

- ✅ **视频壁纸**：支持 MP4 等视频格式作为动态壁纸
- ✅ **硬件加速**：使用 VAAPI 进行硬件解码，降低 CPU 占用
- ✅ **性能优化**：支持帧率控制和分辨率缩放，默认 30fps @ 720p
- ✅ **Wayland 原生**：基于 Wayland 协议，支持 layer-shell 和 viewporter
- ✅ **Client-Daemon 架构**：通过 IPC 通信，支持远程控制
- ⏳ **Web 壁纸**：计划支持 HTML/WebGL 壁纸（开发中）
- ⏳ **场景壁纸**：计划支持 3D 场景壁纸（开发中）

## 系统要求

- **操作系统**：Linux (Wayland)
- **显示服务器**：Wayland (支持 layer-shell 协议)
- **GPU**：支持 VAAPI 的 Intel/AMD GPU 或支持其他硬件加速的 GPU
- **依赖**：
  - FFmpeg (用于视频解码)
  - Wayland 协议库

## 安装

### 从源码编译

```bash
# 克隆仓库
git clone <repository-url>
cd waypaper-rs

# 编译
cargo build --release

# 安装（可选）
sudo cp target/release/waypaper-rs /usr/local/bin/
sudo cp target/release/daemon /usr/local/bin/
```

## 使用方法

### 启动 Daemon

Daemon 是后台服务，负责壁纸的渲染和播放。

```bash
# 启动 daemon
./target/release/daemon

# 日志会输出到 /tmp/daemon.log
```

### 设置壁纸

使用 CLI 工具设置壁纸。传入包含 `project.json` 的目录路径。

```bash
# 设置视频壁纸
./target/release/waypaper-rs set /path/to/wallpaper/directory
```

### project.json 格式

壁纸目录需要包含 `project.json` 文件，格式如下：

```json
{
    "type": "video",
    "file": "video.mp4",
    "title": "壁纸标题",
    "description": "壁纸描述",
    "tags": ["Anime", "Music"]
}
```

**字段说明**：
- `type`：壁纸类型，目前支持 `"video"`
- `file`：视频文件名（相对于目录路径）
- `title`：壁纸标题
- `description`：壁纸描述
- `tags`：标签数组

### 其他命令

```bash
# 获取当前壁纸状态
./target/release/waypaper-rs status

# 获取当前壁纸信息
./target/release/waypaper-rs get

# 关闭 daemon
./target/release/waypaper-rs shutdown
```

## 性能优化

### 默认配置

- **帧率**：30 fps
- **分辨率**：720p (1280x720)
- **硬件加速**：VAAPI (Intel/AMD GPU)

### 自定义配置

可以在代码中修改 `VideoWallpaper` 的配置：

```rust
let mut wallpaper = VideoWallpaper::new(video_path, WallpaperType::Video);
wallpaper.set_target_fps(60);  // 设置为 60fps
wallpaper.set_max_resolution(1920, 1080);  // 设置为 1080p
```

## 架构设计

### Client-Daemon 模式

```
┌─────────────┐         IPC          ┌─────────────┐
│   CLI       │ <─────────────────> │   Daemon    │
│ (waypaper)  │   Unix Socket       │  (daemon)   │
└─────────────┘                      └─────────────┘
                                            │
                                            ▼
                                    ┌─────────────┐
                                    │   Player    │
                                    │  (管理器)    │
                                    └─────────────┘
                                            │
                    ┌───────────────────────┼───────────────────────┐
                    ▼                       ▼                       ▼
            ┌─────────────┐         ┌─────────────┐         ┌─────────────┐
            │   Video     │         │    Web      │         │   Scene     │
            │  Wallpaper  │         │  Wallpaper  │         │  Wallpaper  │
            └─────────────┘         └─────────────┘         └─────────────┘
```

### 核心组件

- **CLI**：命令行工具，负责与用户交互
- **Daemon**：后台服务，负责壁纸渲染和播放
- **Player**：壁纸管理器，统一管理不同类型的壁纸
- **VideoWallpaper**：视频壁纸实现
- **Wayland**：Wayland 协议处理，包括 layer-shell 和 viewporter

## 开发计划

- [ ] Web 壁纸支持
- [ ] 场景壁纸支持
- [ ] 更多硬件加速选项（CUDA、QSV 等）
- [ ] 配置文件支持
- [ ] 播放列表功能
- [ ] 音频支持

## 故障排除

### Daemon 无法启动

检查 Wayland 显示服务器是否运行，并确认支持 layer-shell 协议。

### 视频无法播放

1. 检查视频文件格式是否支持
2. 确认 GPU 支持 VAAPI 硬件加速
3. 查看 `/tmp/daemon.log` 日志文件

### CPU 占用过高

1. 确认硬件加速已启用
2. 降低帧率或分辨率
3. 检查视频文件是否过大

## 许可证

MIT License

## 贡献

欢迎提交 Issue 和 Pull Request！