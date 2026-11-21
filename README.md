# Camo Translator

> [!WARNING]
> **本工具会解包《战争雷霆》的本地游戏资源。** 它从你选择的游戏安装目录读取载具模型、UV 和材质数据，用于生成自定义涂装。

Camo Translator 让共用 UV 的零件也能画上不同图案。它为你放入 `Reused` 对象的零件生成独立 UV 和可导入 Substance 3D Painter 的模型，再通过游戏的 **Camo（迷彩）机制**把各自的底色覆盖到对应零件。Camo 适用于游戏中支持迷彩的材质。

## 三步上手

1. **准备模型。** 把想独立上色的零件合并到一个名为 `Reused` 的对象，保留原 UV 和材质槽。在工具里选择游戏目录、载具和源模型，填写项目保存位置，点击 **生成 Painter 模型**。工具会生成 `Painter.fbx` 或 `Painter.obj`，并创建 `Painter PNG` 文件夹。
2. **在 Substance 3D Painter 绘制。** 用生成的模型新建工程，启用 **UV 平铺工作流（UDIM）**，选择保留每种材质的 UV 平铺布局，关闭自动展开。画完将带 UDIM 编号的 8 位 PNG 导出到项目的 `Painter PNG` 文件夹。[Painter 设置截图](assets/painter-settings.png)
3. **导出到游戏。** 回到工具的 **导出涂装** 页，选择 **使用游戏 UserSkins**，填写输出文件夹名并点击 **导出涂装**。工具会自动创建 `UserSkins` 子文件夹，写入 DDS 贴图和 `.blk` 配置。在游戏内刷新自定义涂装并选用它。

## 致谢

感谢 patodois 为 War Thunder 提供资源格式解析和解包支持。 [wt-formats](https://github.com/patodois/wt-formats/tree/main) · [wt-unpack](https://github.com/patodois/wt-unpack)

## Contributors

- MinatoJT
- Codex

<details>
<summary>命令行与进阶说明</summary>

### 打开桌面工具

双击 `target\release\camo-translator.exe`。工具会自动检测游戏目录，也可以在 **准备模型** 中手动浏览。首次识别游戏安装时，工具扫描本机可用的车型并保存目录。以后启动直接读取缓存，游戏资源有变化时自动重新扫描。车型旁的 **重新扫描** 会立即从所选游戏目录更新车型。

在车型栏按中文、英文或游戏编号搜索。使用窗口底部的 **保存默认路径** 可让下次启动恢复两页的路径。

### 准备 Painter 模型

点击 **生成 Painter 模型** 后，工具会读取所选载具的默认隐藏节点，并用顶点位置和原 UV 匹配源模型。你可以按节点查看源对象、材质槽和 UV 预览，在导出前选择 **保留全部面** 或 **移除默认隐藏面**。后者只移除新生成模型中整面均已确认隐藏的多边形。匹配不明确的面会保留。清单以游戏基础配置为准，随选装、预设或损伤切换的隐藏规则另由游戏处理。命令行默认保留，可传入 `--hidden-faces remove` 选择移除。

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
| `prepare-report.json` | 列出对象、每槽需要的 UDIM、DDS 文件名、UV2 匹配和默认隐藏面的扫描结果。 |

在 Painter 新建工程时启用 **Use UV Tile workflow**，选择按材质保留 UV Tile 布局，使用生成的 UV。每个对象仍可单独选为 Geometry Mask。C4D 输入需要已保存的多边形模型和可运行的 `c4dpy`。OBJ 输入支持带原 UV 的三角面和四边面。工具用实际原 UV 和位置匹配游戏面，以米为单位，并识别常见的坐标轴方向。

### 回写 DDS 和生成涂装配置

在 Painter 导出 8-bit PNG，每个 UDIM 保留对应编号。以豹 2A8 的 Body 为例，工具会识别模型名前缀。使用不含模型名前缀的自定义模板也可以直接读取。

| 贴图 | Painter 输出名 | 示例 |
|---|---|---|
| 颜色 | `$mesh_$textureSet_BaseColor.$udim` | `Painter_Body_BaseColor.1001.png` |
| 金属度 | `$mesh_$textureSet_Metallic.$udim` | `Painter_Body_Metallic.1001.png` |
| 粗糙度 | `$mesh_$textureSet_Roughness.$udim` | `Painter_Body_Roughness.1001.png` |
| 法线 | `$mesh_$textureSet_Normal.$udim` | `Painter_Body_Normal.1001.png` |

```powershell
& $tool export --project '.\projects\Leopard2A8' `
    --slot Body --out (Join-Path $game 'UserSkins\my-first-body') --size 4096 --camo-scale 130 --material
```

- 默认回写所有能对应游戏贴图的材质槽。可重复传入 `--slot` 只导出其中几个。90B OBJ 里的 Body 槽叫 `Body_1`。没有自动对应的材质槽可在导出页选择游戏贴图，命令行使用 `--binding 源槽=游戏贴图`。多个槽对应同一张游戏贴图时会合并回写。可用 `--textures` 指定其他 PNG 目录。
- 加 `--material` 才读取后三张图并输出 `_n.dds`。通道固定为 R = 255 − Roughness、G = Normal.G、B = Metallic、A = Normal.R。
- 在导出页通过 **游戏内 Camo 缩放** 选择游戏的 20% 到 200% 档位，默认 200%。工具按这个比例回写迷彩图，同时把比例写入 `.blk`。游戏加载含 Camo 的涂装时会自动固定为所选比例。命令行使用 `--camo-scale` 设置百分比。
- 默认生成车型对应的 `.blk`。需要 Camo 的材质槽包含固定覆盖图 `_c.dds` 和游戏底图 `_camo_base.dds`。`_c.dds` 的 Alpha 决定固定色和迷彩的交界。`_camo_base.dds` 在 UV 岛和扩边内的 Alpha 统一为 255，让游戏对交界采样时底层保持连续。普通固定材质使用其原始贴图名生成替换配置。没有可缩放迷彩的材质使用固定贴图，复用的原 UV 会继续共用颜色。已有自定义配置时可用 `--alpha camo-white` 导出白色的 Reused 遮罩。
- DDS 是 RGBA 8.8.8.8，单层，无 mipmap。导出时会给 UV 岛留出边缘像素，供游戏过滤采样。`--camo-size` 可以单独指定迷彩图尺寸。可再次导出到已有目录，同名的生成文件更新，其他文件保留。
- `Reused` 和普通对象共用的原 UV 像素使用普通对象的固定贴图，界面和 `export-report.json` 会列出这些冲突。报告也会列出 `Reused` 缺失游戏 UV2 的像素，以及迷彩图中收到不同颜色的像素。

### 验收

`cargo test --locked` 运行无需游戏文件的测试。实车回归测试用两份本地源模型和你指定的游戏目录生成项目，再验证共用原 UV 的面可以导出成两种 Camo 颜色。

```powershell
$env:CAMO_TEST_GAME_ROOT = 'D:\Games\War Thunder'
$env:CAMO_TEST_C4DPY = 'D:\Cinema4D\c4dpy.exe'
$env:CAMO_TEST_LEOPARD = 'D:\Models\Leopard2A8.c4d'
$env:CAMO_TEST_TYPE90B = 'D:\Models\90B fixed.obj'
cargo test --release --locked --test real_models -- --ignored --nocapture
```

Alpha 混合和迷彩比例以游戏内验收为准。

</details>
