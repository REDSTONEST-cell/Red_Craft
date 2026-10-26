# mcmod.buf

Mod 中文名数据库，原样取自 [PCL（Plain Craft Launcher）](https://github.com/PCL-Community/PCL-CE)
的 `Plain Craft Launcher 2/Resources/mcmod.buf`，经 PCL 作者（龙腾猫跃）许可使用，未做修改。
数据原始来源为 [MC 百科（mcmod.cn）](https://www.mcmod.cn)。

该文件不适用本仓库的 MIT 许可证，其权利归原作者所有；如需在其他项目中使用，请自行取得 PCL 作者的许可。

格式：gzip 压缩的 protobuf，`repeated Entry`（字段 1），每条 Entry：
1 = MC 百科 ID（int），2 = 中文名（「中文名 (English Name)」），3 = CurseForge slug，4 = Modrinth slug。
