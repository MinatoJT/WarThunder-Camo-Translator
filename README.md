# Camo Translator

Camo Translator 0.1 是 Windows 上的 Rust 桌面工具，支持豹 2A8 和 90B。它读取你选定的 War Thunder 安装目录，使用游戏原版迷彩 UV2，把 Substance 3D Painter 中分开绘制的颜色回写到原贴图和 `camo.dds`。

## 打开桌面工具

双击 `target\release\camo-translator.exe`。在 **准备模型** 中选择游戏目录、车型和源模型，再设置项目名称和保存位置。在 **导出涂装** 中选择已准备的项目和 Painter 导出的 PNG。处理进度和结果会显示在窗口中。游戏目录留空时会自动检测，并可在准备页重新检测或手动浏览。使用窗口底部的 **保存默认路径** 可让下次启动恢复两页的路径。

从终端重复操作或运行测试时，使用下方的命令行程序。

## 准备 Painter 模型

用 `cargo build --release --locked` 构建。命令行请使用 `target\release\camo-translator-cli.exe`。从工具目录执行下列命令，把路径换成自己的。选择包含 `content` 目录的 War Thunder 安装根目录。

```powershell
$tool = '.\target\release\camo-translator-cli.exe'
$game = 'D:\Games\War Thunder'

& $tool prepare --name Leopard2A8 --vehicle leopard-2a8 --game-root $game `
    --scene 'D:\Models\Leopard2A8.c4d' --c4dpy 'D:\Cinema4D\c4dpy.exe' `
    --out '.\projects\Leopard2A8'

& $tool prepare --name Type90B --vehicle type-90b --game-root $game `
    --scene 'D:\Models\90B fixed.obj' --out '.\projects\Type90B'
```

项目目录保存以下内容。

| 文件 | 用途 |
|---|---|
| `Painter.fbx` 或 `Painter.obj` | 导入 Painter 的模型。OBJ 的 `Painter.mtl` 提供原材质槽名。 |
| `project.json` | 保存对象、材质槽、原 UV、Painter UV 及游戏 UV2 的面角对应。 |
| `prepare-report.json` | 列出对象、每槽需要的 UDIM、DDS 文件名和成功匹配到 UV2 的三角面数。 |

在 Painter 新建工程时启用 **Use UV Tile workflow**，选择按材质保留 UV Tile 布局，使用生成的 UV。每个对象仍可单独选为 Geometry Mask。C4D 输入需要已保存的多边形模型和可运行的 `c4dpy`。OBJ 输入支持带原 UV 的三角面和四边面。工具用实际原 UV 和位置匹配游戏面，以米为单位，并识别常见的 Y 向上前轴方向。

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
    --textures 'D:\Paint\Leopard2A8' --slot Body `
    --out '.\preview\my-first-body' --size 4096 --material
```

- 默认回写所有能对应游戏贴图的材质槽。可重复传入 `--slot` 只导出其中几个。90B OBJ 里的 Body 槽叫 `Body_1`。
- 加 `--material` 才读取后三张图并输出 `_n.dds`。通道固定为 R = 255 − Roughness、G = Normal.G、B = Metallic、A = Normal.R。
- 默认 `--alpha fixed-white` 使用游戏 `set_tex` 的透明度规则，固定颜色 Alpha 为白，交给 `camo.dds` 的复用区域为黑。随 DDS 生成 `germ_leopard_2a8.blk` 或 `jp_type_90b.blk`。已有自定义配置时可用 `--alpha camo-white` 导出复用区域为白的 DDS。
- DDS 是 RGBA 8.8.8.8，单层，无 mipmap。`--camo-size` 可以单独指定迷彩图尺寸。输出目录每次用新名字。
- `export-report.json` 记录送入迷彩的复用像素、缺 UV2 的复用像素及 UV2 上出现多种颜色的像素。发生 UV2 冲突时优先保留需要区分的颜色，其次取靠外的面。分离的玻璃槽可在 Painter 使用，豹 2A8 的玻璃替换规则留待后续版本。

## 验收

`cargo test --locked` 运行无需游戏文件的测试。实车回归测试用两份本地源模型和你指定的游戏目录生成项目，再验证共用原 UV 的面可以导出成两种 Camo 颜色。

```powershell
$env:CAMO_TEST_GAME_ROOT = 'D:\Games\War Thunder'
$env:CAMO_TEST_C4DPY = 'D:\Cinema4D\c4dpy.exe'
$env:CAMO_TEST_LEOPARD = 'D:\Models\Leopard2A8.c4d'
$env:CAMO_TEST_TYPE90B = 'D:\Models\90B fixed.obj'
cargo test --release --locked --test real_models -- --ignored --nocapture
```

把生成的 `.blk` 和同一批次的 DDS 放在游戏 `UserSkins` 的一个新子目录，刷新涂装列表后选择该涂装。Alpha 混合和迷彩比例以游戏内验收为准。本地项目、测试 DDS 和原始模型都留在本机。每轮按用户的验收结果提交并推送源码。
