# Waypaper-rs

A dynamic wallpaper application for Wayland written in Rust, supporting video wallpaper playback.

> **Note**: Most of the code in this project was generated with AI assistance.

## Project Goal

This project aims to implement Wallpaper Engine-like wallpaper playback functionality on Linux using Rust, allowing users to use Wallpaper Engine wallpaper resources on Linux.

## Features

- ✅ **Video Wallpapers**: Support MP4 and other video formats as dynamic wallpapers
- ✅ **Hardware Acceleration**: Use VAAPI for hardware decoding to reduce CPU usage
- ✅ **Performance Optimization**: Support frame rate control and resolution scaling, default 30fps @ 720p
- ✅ **Wayland Native**: Based on Wayland protocol with layer-shell and viewporter support
- ✅ **Client-Daemon Architecture**: IPC communication with remote control support
- ⏳ **Web Wallpapers**: Planned support for HTML/WebGL wallpapers (in development)
- ⏳ **Scene Wallpapers**: Planned support for 3D scene wallpapers (in development)

## System Requirements

- **OS**: Linux (Wayland)
- **Display Server**: Wayland (with layer-shell protocol support)
- **GPU**: Intel/AMD GPU with VAAPI support or other GPU with hardware acceleration
- **Dependencies**:
  - FFmpeg (for video decoding)
  - Wayland protocol libraries

## Installation

### Build from Source

```bash
# Clone the repository
git clone <repository-url>
cd waypaper-rs

# Build
cargo build --release

# Install (optional)
sudo cp target/release/waypaper-rs /usr/local/bin/
sudo cp target/release/daemon /usr/local/bin/
```

## Usage

### Start Daemon

The daemon is a background service responsible for wallpaper rendering and playback.

```bash
# Start daemon
./target/release/daemon

# Logs are written to stdout, can be redirected
./target/release/daemon > /tmp/daemon.log 2>&1
```

### Set Wallpaper

Use the CLI tool to set wallpaper. Provide the directory path containing `project.json`. This project is compatible with the format of Wallpaper Engine workshop downloads.

```bash
# Set video wallpaper
./target/release/waypaper-rs set /path/to/wallpaper/directory
```

**Note**: Wallpaper Engine workshop downloads typically include a `project.json` file and can be used directly.

### project.json Format

The wallpaper directory must contain a `project.json` file with the following format:

```json
{
    "type": "video",
    "file": "video.mp4",
    "title": "Wallpaper Title",
    "description": "Wallpaper description",
    "tags": ["Anime", "Music"]
}
```

**Field Descriptions**:
- `type`: Wallpaper type, currently supports `"video"`
- `file`: Video filename (relative to directory path)
- `title`: Wallpaper title
- `description`: Wallpaper description
- `tags`: Array of tags

### Other Commands

```bash
# Get current wallpaper status
./target/release/waypaper-rs status

# Get current wallpaper info
./target/release/waypaper-rs get

# Shutdown daemon
./target/release/waypaper-rs shutdown
```

## Performance Optimization

### Default Configuration

- **Frame Rate**: 30 fps
- **Resolution**: 1080p (1920x1080)
- **Hardware Acceleration**: VAAPI (Intel/AMD GPU)

### Custom Configuration

You can modify `VideoWallpaper` configuration in the code:

```rust
let mut wallpaper = VideoWallpaper::new(video_path, WallpaperType::Video);
wallpaper.set_target_fps(60);  // Set to 60fps
wallpaper.set_max_resolution(2560, 1440);  // Set to 2K
```

## Architecture Design

### Client-Daemon Mode

```
┌─────────────┐         IPC          ┌─────────────┐
│   CLI       │ <─────────────────> │   Daemon    │
│ (waypaper)  │   Unix Socket       │  (daemon)   │
└─────────────┘                      └─────────────┘
                                            │
                                            ▼
                                    ┌─────────────┐
                                    │   Player    │
                                    │  (Manager)  │
                                    └─────────────┘
                                            │
                    ┌───────────────────────┼───────────────────────┐
                    ▼                       ▼                       ▼
            ┌─────────────┐         ┌─────────────┐         ┌─────────────┐
            │   Video     │         │    Web      │         │   Scene     │
            │  Wallpaper  │         │  Wallpaper  │         │  Wallpaper  │
            └─────────────┘         └─────────────┘         └─────────────┘
```

### Core Components

- **CLI**: Command-line tool for user interaction
- **Daemon**: Background service for wallpaper rendering and playback
- **Player**: Wallpaper manager that manages different wallpaper types
- **VideoWallpaper**: Video wallpaper implementation
- **Wayland**: Wayland protocol handling, including layer-shell and viewporter

## Development Roadmap

- [ ] Web wallpaper support
- [ ] Scene wallpaper support
- [ ] More hardware acceleration options (CUDA, QSV, etc.)
- [ ] Configuration file support
- [ ] Playlist functionality
- [ ] Audio support

## Troubleshooting

### Daemon Won't Start

Check if Wayland display server is running and confirm layer-shell protocol support.

### Video Won't Play

1. Check if video format is supported
2. Confirm GPU supports VAAPI hardware acceleration
3. Check `/tmp/daemon.log` log file

### High CPU Usage

1. Confirm hardware acceleration is enabled
2. Reduce frame rate or resolution
3. Check if video file is too large

## License

MIT License

## Contributing

Issues and Pull Requests are welcome!