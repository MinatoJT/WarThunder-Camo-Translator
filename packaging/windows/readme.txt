Camo Translator

在 Windows 10 或 11 x64 安装。桌面界面需要支持 Direct3D 12 的显卡驱动。
安装后从开始菜单打开 Camo Translator。用右上角的菜单切换简体中文或 English。

本工具会解包《战争雷霆》的本地游戏资源。

1. 选择 War Thunder 安装目录和车型。在源模型里，将需要用 Camo 独立上色的面放入 Reused 对象，保留原 UV 和材质槽。使用 OBJ 可以直接导入，打开 C4D 文件需要现有 Cinema 4D 安装中的 c4dpy.exe。
2. 在工具里生成 Painter 模型，在 Substance 3D Painter 中启用 UV 平铺工作流，保留每种材质的 UV 平铺布局。法线贴图格式选择 DirectX。
3. 将 8 位 PNG 导出到项目的 Painter PNG 目录，保留材质槽名和 UDIM 编号。回到工具的「导出涂装」生成 DDS 和 BLK，在游戏里刷新自定义涂装。导出材质需要 Metallic、Roughness、Normal 三张 PNG。

在「导出涂装」中可选用 DDS Builder 的色彩补偿预设，或勾选「去除载具贴纸」替换使用 Camo 遮罩的材质底图。去除贴纸时会一并打包对应的材质通道。

如果反馈问题，请附上车型、导出时选的 Camo 缩放比例、导出报告和游戏内截图。
