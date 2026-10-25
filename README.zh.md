# AutoGameSoftwareSync (AGSS)

[English](README.md) | [Français](README.fr.md) | [Русский](README.ru.md) | [Español](README.es.md) | **中文**

你的游戏和软件存档与设置，通过任意共享文件夹（Google Drive、Dropbox、NAS 等）在多台电脑（台式机、Legion Go S 等掌机）之间同步。个人使用免费。

[**下载最新版本**](https://github.com/Epikou/AutoGameSoftwareSync/releases/latest) · [在 Ko-fi 上支持本项目](https://ko-fi.com/autogamesoftwaresave)

## 功能

- 关闭游戏时自动备份存档，并在另一台电脑上恢复最新版本。
- 绝不丢失数据：每次备份都是独立的新文件夹，旧版本始终保留，每次恢复之前都会先做安全备份。
- 自动识别你的游戏（Steam、Epic、GOG），并收录数千个存档位置，也支持手动添加。
- 静默运行在系统托盘，可随 Windows 启动（包括掌机上的 Xbox 全屏体验）。
- 完整的手柄操作支持（DualSense、Xbox 手柄）。语言：English、Français、Русский、Español、中文。

## 使用方法

1. 从 [Releases](https://github.com/Epikou/AutoGameSoftwareSync/releases) 页面下载 `AutoGameSoftwareSync.exe`，放在一个固定文件夹中（例如 `C:\Tools\AutoGameSoftwareSync`）。
2. 运行程序。简短的引导教程会逐步说明：选择共享文件夹、添加第一个游戏、调整设置。
3. 在另一台电脑上使用**相同的共享文件夹**重复上述步骤，并对已有游戏点击**加入**。

如果共享文件夹是 Google Drive，请将其设为*镜像文件*（或将该文件夹设为*可离线使用*），否则读取可能非常缓慢。

## Linux / SteamOS

同时提供 Linux 版本（x86_64）：SteamOS（Steam Deck、Legion Go S 等）、Bazzite 和 Ubuntu。请从 [Releases](https://github.com/Epikou/AutoGameSoftwareSync/releases/latest) 页面下载 `AutoGameSoftwareSync-v1.3.3-linux-x86_64.tar.gz`，解压后运行 `install-linux.sh`（无需管理员权限）。通过同一个共享文件夹，存档与 Windows 版完全兼容。安装指南（法语）：[docs/LINUX.md](docs/LINUX.md)。

**尚未在真实的 SteamOS 上测试**（目前仅在 Ubuntu 上测试）：非常欢迎反馈，请提交 [issue](https://github.com/Epikou/AutoGameSoftwareSync/issues)。

## Windows SmartScreen 提示

程序目前尚未进行代码签名。首次运行时 Windows 可能显示“Windows 已保护你的电脑”。请点击**更多信息**，然后点击**仍要运行**。随着使用者增多，该提示会逐渐消失。

## 安全提示

首次使用前请手动备份你的存档。本程序按“现状”提供，不提供任何担保（见 [TERMS.md](TERMS.md)）。

## 已知限制

如果游戏不是通过集成启动器启动，程序无法在游戏启动*之前*恢复存档。计划为此提供 Playnite 扩展。

## 关于这个项目

AutoGameSoftwareSync 是用“氛围编程”（vibe coding）做出来的：我感受氛围，所以我写代码。对游戏和电脑的热爱已经超过 40 年 <3

它出自热情，而不是一个庞大的团队，所以难免有些粗糙的地方（偶尔还有让我扶额的 bug）。欢迎任何帮助、建议、错误报告或想法：请提交 [issue](https://github.com/Epikou/AutoGameSoftwareSync/issues)。感谢你的到来！

## 支持

问题与建议：请提交 [issue](https://github.com/Epikou/AutoGameSoftwareSync/issues)。如果你喜欢这个项目，可以[请我们喝杯咖啡](https://ko-fi.com/autogamesoftwaresave)。

*本译文为自动翻译，可能存在错误。*
