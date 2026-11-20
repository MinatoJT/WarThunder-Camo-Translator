# Camo Translator 0.1 实施计划

第一版用同一条 Rust 流程处理豹 2A8 的 C4D 与 90B 的 OBJ。用户在 `prepare` 时指定游戏根目录和车型。验收时逐车用已共用原 UV 的左右对应面检查独立颜色，并验证输出通道。

| 阶段 | 实现和验收 |
|---|---|
| 读取并保存项目 | `vehicle.rs` 在用户选择的安装目录定位资源。`game.rs` 读取原版 UV2 并按位置与原 UV 匹配。`project.json` 保存三个 UV 布局和对象、材质槽对应。 |
| 生成 Painter 模型 | `atlas.rs` 保留普通部件的共用 UV，并为用户标记的 `Reused` 安排独立 UDIM。C4D 适配器导出保留网格和材质槽的 FBX，Rust 恢复网格原名。OBJ 保留顶点、法线、对象和材质槽。 |
| 回写颜色 | `bake.rs` 按面角回写原 UV，仅把 `Reused` 送入原版迷彩 UV2。固定图的 Alpha 控制固定色和迷彩的交界，游戏底图的 Alpha 在 UV 岛及扩边内保持 255。报告和普通对象共用原 UV、缺少 UV2 或迷彩 UV2 互相覆盖的情况。 |
| 打包材质 | `dds.rs` 写 8.8.8.8、无 mipmap DDS。用户验证的固定通道为 R = 255 − Roughness、G = Normal.G、B = Metallic、A = Normal.R。材质可继续共用原 UV。 |
| 验证两种模型 | `tests/workflow.rs` 核对基本拆叠、通道和颜色回写。`tests/real_models.rs` 从自选游戏根目录跑两辆实车，测同一原 UV 的两个迷彩像素分别取到不同颜色。 |
| 用户验收 | 核对两份 Painter 模型。在生成的 Painter 模型上绘制并导出贴图，回写 DDS 和车型 `.blk`，在游戏里检查颜色对应、Alpha 混合及迷彩比例。验收后把本地源码提交并推送到用户的 GitHub。 |
