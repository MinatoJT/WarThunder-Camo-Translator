# Camo Translator

Camo Translator 0.2 是 Windows 上的 Rust 桌面工具。它读取你选定的 War Thunder 安装目录，列出可用的地面、航空和舰艇载具，再把 Substance 3D Painter 绘制的贴图回写成游戏的 DDS 和 BLK。支持可缩放迷彩的材质使用游戏原版 UV2。

## 打开桌面工具

双击 `target\release\camo-translator.exe`。工具会自动检测游戏目录，也可以在 **准备模型** 中手动浏览。首次识别游戏安装时，工具扫描本机可用的车型并保存目录。以后启动直接读取缓存，游戏资源有变化时自动重新扫描。车型旁的 **重新扫描** 会立即从所选游戏目录更新车型。

在车型栏按中文、英文或游戏编号搜索，选中车型后填写已修完的模型、项目名称和项目保存位置。生成项目后，导出页会自动填入项目下的 `Painter PNG` 和游戏的 `UserSkins`，也可以手动选择这两个目录。处理进度和结果会显示在窗口中。使用窗口底部的 **保存默认路径** 可让下次启动恢复两页的路径。

从终端重复操作或运行测试时，使用下方的命令行程序。

## 准备 Painter 模型

在源模型里，把需要独立绘制并交给游戏迷彩图的面放入名为 `Reused` 的网格对象或组，保留它们的原 UV 和材质槽。例如把裙板需要单独画的外表面放入 `Reused`，让烟雾弹等部件继续共用 UV。工具准备 Painter 网格时只为 `Reused` 拆开重叠，普通部件保留共用关系。

用 `cargo build --release --locked` 构建。命令行请使用 `target\release\camo-translator-cli.exe`。从工具目录执行下列命令，把路径换成自己的。选择包含 `content` 目录的 War Thunder 安装根目录。

```powershell
$tool = '.\target\release\camo-translator-cli.exe'
$game = 'D:\Games\War Thunder'

& $tool vehicles --game-root $game 'Leopard 2A8'

& $tool prepare --name Leopard2A8 --vehicle germ_leopard_2a8 --game-root $game `
    --scene 'D:\Models\Leopard2A8.c4d' --c4dpy 'D:\Cinema4D\c4dpy.exe' `
    --out '.\projects\Leopard2A8'

& $tool prepare --name Type90B --vehicle jp_type_90b --game-root $game `
    --scene 'D:\Models\90B fixed.obj' --out '.\projects\Type90B'
```

项目目录保存以下内容。

| 文件 | 用途 |
|---|---|
| `Painter.fbx` 或 `Painter.obj` | 导入 Painter 的模型。OBJ 的 `Painter.mtl` 提供原材质槽名。 |
| `Painter PNG` | Painter 的 PNG 导出目录。 |
| `project.json` | 保存对象、材质槽、载具贴图绑定及原 UV、Painter UV 和游戏 UV2 的对应。 |
| `prepare-report.json` | 列出对象、每槽需要的 UDIM、DDS 文件名和成功匹配到 UV2 的三角面数。 |

在 Painter 新建工程时启用 **Use UV Tile workflow**，选择按材质保留 UV Tile 布局，使用生成的 UV。每个对象仍可单独选为 Geometry Mask。C4D 输入需要已保存的多边形模型和可运行的 `c4dpy`。OBJ 输入支持带原 UV 的三角面和四边面。工具用实际原 UV 和位置匹配游戏面，以米为单位，并识别常见的坐标轴方向。

## 回写 DDS 和生成涂装配置

在 Painter 导出 8-bit PNG，每个 UDIM 保留对应编号。以豹 2A8 的 Body 为例，工具会识别模型名前缀。使用不含模型名前缀的自定义模板也可以直接读取。

| 贴图 | Painter 输出名 | 示例 |
|---|---|---|
| 颜色 | `$mesh_$textureSet_BaseColor.$udim` | `Painter_Body_BaseColor.1001.png` |
| 金属度 | `$mesh_$textureSet_Metallic.$udim` | `Painter_Body_Metallic.1001.png` |
| 粗糙度 | `$mesh_$textureSet_Roughness.$udim` | `Painter_Body_Roughness.1001.png` |
| 法线 | `$mesh_$textureSet_Normal.$udim` | `Painter_Body_Normal.1001.png` |

```powershell
& $tool export --project '.\projects\Leopard2A8' `
    --slot Body --out (Join-Path $game 'UserSkins\my-first-body') --size 4096 --material
```

- 默认回写所有能对应游戏贴图的材质槽。可重复传入 `--slot` 只导出其中几个。90B OBJ 里的 Body 槽叫 `Body_1`。没有自动对应的材质槽可在导出页选择游戏贴图，命令行使用 `--binding 源槽=游戏贴图`。多个槽对应同一张游戏贴图时会合并回写。可用 `--textures` 指定其他 PNG 目录。
- 加 `--material` 才读取后三张图并输出 `_n.dds`。通道固定为 R = 255 − Roughness、G = Normal.G、B = Metallic、A = Normal.R。
- 默认生成车型对应的 `.blk`。需要 Camo 的材质槽包含固定覆盖图 `_c.dds` 和游戏底图 `_camo_base.dds`。`_c.dds` 的 Alpha 决定固定色和迷彩的交界。`_camo_base.dds` 在 UV 岛和扩边内的 Alpha 统一为 255，让游戏对交界采样时底层保持连续。普通固定材质使用其原始贴图名生成替换配置。没有可缩放迷彩的材质使用固定贴图，复用的原 UV 会继续共用颜色。已有自定义配置时可用 `--alpha camo-white` 导出白色的 Reused 遮罩。
- DDS 是 RGBA 8.8.8.8，单层，无 mipmap。导出时会给 UV 岛留出边缘像素，供游戏过滤采样。`--camo-size` 可以单独指定迷彩图尺寸。可再次导出到已有目录，同名的生成文件更新，其他文件保留。
- `Reused` 和普通对象共用的原 UV 像素使用普通对象的固定贴图，界面和 `export-report.json` 会列出这些冲突。报告也会列出 `Reused` 缺失游戏 UV2 的像素，以及迷彩图中收到不同颜色的像素。

## 验收

`cargo test --locked` 运行无需游戏文件的测试。实车回归测试用两份本地源模型和你指定的游戏目录生成项目，再验证共用原 UV 的面可以导出成两种 Camo 颜色。

```powershell
$env:CAMO_TEST_GAME_ROOT = 'D:\Games\War Thunder'
$env:CAMO_TEST_C4DPY = 'D:\Cinema4D\c4dpy.exe'
$env:CAMO_TEST_LEOPARD = 'D:\Models\Leopard2A8.c4d'
$env:CAMO_TEST_TYPE90B = 'D:\Models\90B fixed.obj'
cargo test --release --locked --test real_models -- --ignored --nocapture
```

把生成的 `.blk` 和同一批次的 DDS 导出到游戏 `UserSkins` 的子目录，刷新涂装列表后选择该涂装。Alpha 混合和迷彩比例以游戏内验收为准。本地项目、测试 DDS 和原始模型都留在本机。每轮按用户的验收结果提交并推送源码。
